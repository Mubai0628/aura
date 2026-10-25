mod attachment;
mod capabilities;
pub(crate) use attachment::AttachmentGrant;
mod classification;
mod file_origin;
mod namespace;
use namespace::relative_under;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use aura_domain_common::{
    ApprovalRequestId, CorrelationId, ExternalGrantId, FileIdentity, SessionId, TurnTaskId,
    UtcTimestamp, WorkspaceId, WorkspacePath,
};
use aura_permission_domain::{
    AttachExternalReadOnlyInput, DeriveWorkspaceExternalGrantInput, ExternalGrantCapability,
    ExternalGrantLifecycle, ExternalGrantRecord, ExternalGrantTargetKind, ExternalGrantTerm,
    ExternalGrantView, PermissionGrant, ResolvedScope,
};
use aura_persistence::{PersistenceCoordinator, PersistenceError};
use aura_security_contract::{
    ApprovalDecisionKind, CanonicalPath, PermissionGrantLifetime, ResolvedCapability,
};
use aura_windows_platform::{
    NativeFileAccess, NativePathInfo, VerifiedFile, final_path_within, inspect_existing_path,
    open_verified_existing, open_verified_publish_parent,
};
use aura_workspace::domain_identity;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExternalGrantError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("external path must be an existing absolute path")]
    InvalidSelection,
    #[error("the external path approval does not contain a verified file identity")]
    IdentityUnavailable,
    #[error("the external path identity changed and requires a new authorization")]
    IdentityChanged,
    #[error("the external grant is suspended until its identity is revalidated")]
    Suspended,
    #[error("the external grant was revoked")]
    Revoked,
    #[error("the external grant has expired")]
    Expired,
    #[error("the external grant does not provide the required capability")]
    CapabilityUnavailable,
    #[error("the target is outside the recursive external grant boundary")]
    ScopeUnavailable,
    #[error("the workspace permission grant cannot be reused for this session")]
    PermissionGrantUnavailable,
    #[error("external path resolution failed: {0}")]
    PathResolution(#[from] std::io::Error),
}

pub struct ResolvedExternalFile {
    pub grant: ExternalGrantRecord,
    pub file: VerifiedFile,
}

/// The active grant one absolute path belongs to, and the path inside it.
///
/// Classification only: it says which authorized root a caller's spelling refers
/// to so the operation can be built against that root. The permission engine then
/// checks the resource and the file layer re-verifies the capability, the scope and
/// the file identity on the handle it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoveredExternalPath {
    pub grant_id: ExternalGrantId,
    /// The grant root as the user selected it, for display and audit.
    pub root_display: String,
    /// The path relative to that root, which is the form the grant scope is
    /// checked against.
    pub relative_path: WorkspacePath,
    /// The candidate's own verified **final** path, as the platform resolved it.
    ///
    /// It is carried out of classification so a caller never has to re-derive which object a
    /// request reaches from the model's spelling. The protection decision must be made about
    /// the object that will really be acted on: re-reading the caller's text would let an
    /// alias, a link or a target replaced between the two reads step over that decision.
    pub canonical_path: CanonicalPath,
}

pub struct ValidatedExternalParent {
    pub grant: ExternalGrantRecord,
    pub parent: VerifiedFile,
    pub leaf_name: std::ffi::OsString,
    pub parent_identity: FileIdentity,
    pub final_destination: CanonicalPath,
}

#[derive(Clone)]
pub struct ExternalGrantService {
    persistence: Arc<PersistenceCoordinator>,
    /// The agent journal, when the process has one. The management list uses it
    /// to tell a turn grant that is still the current turn from one whose turn ended.
    turn_journal: Arc<std::sync::Mutex<Option<Arc<crate::broker::BrokerJournal>>>>,
}

impl ExternalGrantService {
    #[must_use]
    pub fn new(persistence: Arc<PersistenceCoordinator>) -> Self {
        Self { persistence, turn_journal: Arc::new(std::sync::Mutex::new(None)) }
    }

    /// Connects the management list to the agent journal that knows the current turn.
    pub fn bind_turn_journal(&self, journal: Arc<crate::broker::BrokerJournal>) {
        *self.turn_journal.lock().unwrap_or_else(|error| error.into_inner()) = Some(journal);
    }

    /// The agent journal this service was bound to, if any.
    ///
    /// The V2 Store behind it owns the authorization facts — reusable Native
    /// session consents included — so the management surface reads them from the
    /// same store the dispatch guard reads rather than from a second copy.
    #[must_use]
    pub fn bound_journal(&self) -> Option<Arc<crate::broker::BrokerJournal>> {
        self.turn_journal.lock().unwrap_or_else(|error| error.into_inner()).clone()
    }

    pub fn derive_workspace_permission(
        &self,
        input: DeriveWorkspaceExternalGrantInput,
        correlation_id: CorrelationId,
    ) -> Result<Vec<ExternalGrantView>, ExternalGrantError> {
        self.ensure_session_workspace(input.session_id, input.workspace_id)?;
        let permission = self.persistence.permission_grant(input.permission_grant_id)?;
        self.ensure_reusable_workspace_permission(&permission, input.workspace_id)?;
        let capabilities = external_capabilities(&permission);
        if capabilities.is_empty() {
            return Err(ExternalGrantError::PermissionGrantUnavailable);
        }
        let mut records = Vec::new();
        for scope in &permission.scopes {
            let ResolvedScope::ExternalPath { canonical_path: approved_path, identity, recursive } =
                scope
            else {
                continue;
            };
            let expected_identity =
                identity.as_ref().ok_or(ExternalGrantError::IdentityUnavailable)?;
            let info = inspect_existing_path(&PathBuf::from(&approved_path.display), true)?;
            if domain_identity(&info.identity) != *expected_identity {
                return Err(ExternalGrantError::IdentityChanged);
            }
            if *recursive && !info.is_directory {
                return Err(ExternalGrantError::ScopeUnavailable);
            }
            if let Some(existing) =
                self.persistence.list_session_external_grants(input.session_id)?.into_iter().find(
                    |record| {
                        record.permission_grant_id == Some(input.permission_grant_id)
                            && record.root_identity == *expected_identity
                            && record.lifecycle != ExternalGrantLifecycle::Revoked
                    },
                )
            {
                records.push(ExternalGrantView::from(
                    &self.revalidate_record(existing, correlation_id)?,
                ));
                continue;
            }
            let now = UtcTimestamp::now();
            let record = ExternalGrantRecord {
                id: ExternalGrantId::new(),
                session_id: input.session_id,
                workspace_id: input.workspace_id,
                permission_grant_id: Some(input.permission_grant_id),
                selected_root: approved_path.clone(),
                canonical_root: canonical_path(info.final_path.display().to_string()),
                root_identity: expected_identity.clone(),
                target_kind: if info.is_directory {
                    ExternalGrantTargetKind::Directory
                } else {
                    ExternalGrantTargetKind::File
                },
                capabilities: capabilities.clone(),
                recursive_scope: *recursive,
                lifecycle: ExternalGrantLifecycle::Active,
                revalidation_revision: 1,
                created_at: now,
                updated_at: now,
                suspended_at: None,
                revoked_at: None,
                expires_at: None,
                turn_task_id: None,
                term: ExternalGrantTerm::Session,
            };
            records.push(ExternalGrantView::from(
                &self.persistence.create_external_grant(record, correlation_id)?,
            ));
        }
        if records.is_empty() {
            return Err(ExternalGrantError::PermissionGrantUnavailable);
        }
        Ok(records)
    }

    pub fn list_session(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<ExternalGrantView>, ExternalGrantError> {
        let records = self.records_for_classification(
            session_id,
            self.persistence.workspace_of_session(session_id)?,
        )?;
        Ok(records
            .iter()
            .map(|record| {
                let mut view = ExternalGrantView::from(record);
                view.availability = aura_permission_domain::project_grant_availability(
                    record,
                    UtcTimestamp::now(),
                    self.turn_grant_is_open(record),
                );
                view
            })
            .collect())
    }

    /// A turn grant is open only while its turn is the session's current, non-terminal turn.
    ///
    /// Without a journal the list cannot see the turn, so an unbound service leaves the
    /// clock decision in place. Once the journal is bound, a missing or finished turn
    /// is not in force.
    fn turn_grant_is_open(&self, record: &ExternalGrantRecord) -> bool {
        if record.term != ExternalGrantTerm::Turn {
            return true;
        }
        let Some(turn_task_id) = record.turn_task_id else { return false };
        let journal = self.turn_journal.lock().unwrap_or_else(|error| error.into_inner()).clone();
        let Some(journal) = journal else { return true };
        let Ok(store) = journal.lock() else { return false };
        let db = store.database.connection();
        let active = aura_persistence_v2::agent::session::load(db, record.session_id)
            .ok()
            .and_then(|state| state.active_turn);
        if active != Some(turn_task_id) {
            return false;
        }
        let state: Option<String> = db
            .query_row(
                "SELECT state FROM turn_tasks WHERE id=?1",
                [turn_task_id.to_string()],
                |row| row.get(0),
            )
            .ok();
        matches!(
            state.as_deref(),
            Some("pending" | "running" | "awaiting_approval" | "awaiting_query")
        )
    }

    /// Mint or reuse the directory grant one user approval froze.
    ///
    /// `approval_id` is that decision. Replaying it returns the grant it already bound,
    /// even after the user revoked that grant, so the replay cannot undo the revocation.
    /// A different approval may reuse a grant that is still active, or mint a new one
    /// after revocation. Allow-once never reaches here. The confirmation window on the
    /// approval request is not copied onto the grant.
    pub fn issue_approved_directory(
        &self,
        spec: ApprovedDirectoryGrant,
        approval_id: ApprovalRequestId,
        operation_fingerprint: &str,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantRecord, ExternalGrantError> {
        if spec.capabilities.is_empty()
            || (spec.term == ExternalGrantTerm::Turn && spec.turn_task_id.is_none())
            || (spec.term != ExternalGrantTerm::Turn && spec.turn_task_id.is_some())
        {
            return Err(ExternalGrantError::InvalidSelection);
        }
        let now = UtcTimestamp::now();
        if spec.expires_at.is_some_and(|expiry| expiry <= now) {
            return Err(ExternalGrantError::Expired);
        }
        let record = ExternalGrantRecord {
            id: ExternalGrantId::new(),
            session_id: spec.session_id,
            workspace_id: spec.workspace_id,
            permission_grant_id: None,
            selected_root: spec.canonical_root.clone(),
            canonical_root: spec.canonical_root,
            root_identity: spec.root_identity,
            target_kind: ExternalGrantTargetKind::Directory,
            capabilities: spec.capabilities,
            recursive_scope: spec.recursive_scope,
            lifecycle: ExternalGrantLifecycle::Active,
            revalidation_revision: 1,
            created_at: now,
            updated_at: now,
            suspended_at: None,
            revoked_at: None,
            expires_at: spec.expires_at,
            turn_task_id: spec.turn_task_id,
            term: spec.term,
        };
        Ok(self.persistence.issue_bound_directory_grant(
            approval_id,
            operation_fingerprint.to_owned(),
            record,
            correlation_id,
        )?)
    }

    /// Whether every directory grant bound to this frozen operation is still in force.
    /// No binding means this operation did not mint a directory grant.
    pub fn bound_directory_grants_in_force(
        &self,
        operation_fingerprint: &str,
    ) -> Result<bool, ExternalGrantError> {
        let now = UtcTimestamp::now();
        for grant_id in self.persistence.directory_grants_for_operation(operation_fingerprint)? {
            let record = self.record(grant_id)?;
            let live = record.lifecycle == ExternalGrantLifecycle::Active
                && record.expires_at.is_none_or(|expiry| expiry > now);
            if !live {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn records_for_classification(
        &self,
        session_id: SessionId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ExternalGrantRecord>, ExternalGrantError> {
        let mut records = self.persistence.list_session_external_grants(session_id)?;
        for record in self.persistence.list_workspace_external_grants(workspace_id)? {
            if records.iter().any(|existing| existing.id == record.id) {
                continue;
            }
            records.push(record);
        }
        Ok(records)
    }

    /// The stored record of one grant. Reading it is not authorization: the caller
    /// uses it for identity display and audit, and the capability, lifecycle and
    /// scope checks happen in [`Self::resolve_existing`].
    pub fn record(
        &self,
        grant_id: ExternalGrantId,
    ) -> Result<ExternalGrantRecord, ExternalGrantError> {
        Ok(self.persistence.external_grant(grant_id)?)
    }

    /// Revalidates every suspended ExternalGrant belonging to a restored
    /// Workspace. Missing/unavailable roots remain suspended (fail closed);
    /// changed identities and invalid backing PermissionGrants are revoked.
    pub fn revalidate_suspended_for_workspace(
        &self,
        workspace_id: WorkspaceId,
        correlation_id: CorrelationId,
    ) -> Result<(), ExternalGrantError> {
        let sessions = self
            .persistence
            .workspace_session_projection()?
            .sessions
            .into_iter()
            .filter(|session| session.workspace_id == workspace_id)
            .map(|session| session.session_id)
            .collect::<Vec<_>>();
        for session_id in sessions {
            for record in self.persistence.list_session_external_grants(session_id)? {
                if record.lifecycle != ExternalGrantLifecycle::Suspended {
                    continue;
                }
                match self.revalidate_record(record, correlation_id) {
                    Ok(_) => {}
                    Err(ExternalGrantError::Persistence(error)) => return Err(error.into()),
                    Err(
                        ExternalGrantError::IdentityChanged
                        | ExternalGrantError::PermissionGrantUnavailable
                        | ExternalGrantError::PathResolution(_)
                        | ExternalGrantError::ScopeUnavailable
                        | ExternalGrantError::IdentityUnavailable
                        | ExternalGrantError::InvalidSelection
                        | ExternalGrantError::Suspended
                        | ExternalGrantError::Revoked
                        | ExternalGrantError::Expired
                        | ExternalGrantError::CapabilityUnavailable,
                    ) => {
                        // The record remains suspended or was explicitly revoked.
                    }
                }
            }
        }
        Ok(())
    }

    pub fn revalidate(
        &self,
        grant_id: ExternalGrantId,
        session_id: SessionId,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantView, ExternalGrantError> {
        let record = self.persistence.external_grant(grant_id)?;
        if record.session_id != session_id {
            return Err(PersistenceError::ExternalGrantSessionMismatch.into());
        }
        Ok(ExternalGrantView::from(&self.revalidate_record(record, correlation_id)?))
    }

    pub fn revoke(
        &self,
        grant_id: ExternalGrantId,
        session_id: SessionId,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantView, ExternalGrantError> {
        let record =
            self.persistence.revoke_external_grant(grant_id, session_id, correlation_id)?;
        Ok(ExternalGrantView::from(&record))
    }

    pub fn resolve_existing(
        &self,
        grant_id: ExternalGrantId,
        session_id: SessionId,
        relative_path: &WorkspacePath,
        capability: ExternalGrantCapability,
        access: NativeFileAccess,
        correlation_id: CorrelationId,
    ) -> Result<ResolvedExternalFile, ExternalGrantError> {
        let record = self.verify_active(grant_id, session_id, capability, correlation_id)?;
        let root = self.inspect_current_root(&record, correlation_id)?;
        let target = target_path(&record, relative_path)?;
        let file = open_verified_existing(&target, access)?;
        if !within_record(&record, &root, file.info(), relative_path) {
            return Err(ExternalGrantError::ScopeUnavailable);
        }
        Ok(ResolvedExternalFile { grant: record, file })
    }

    pub fn verify_published_file(
        &self,
        grant_id: ExternalGrantId,
        session_id: SessionId,
        relative_path: &WorkspacePath,
        capability: ExternalGrantCapability,
        published: &NativePathInfo,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantRecord, ExternalGrantError> {
        let record = self.verify_active(grant_id, session_id, capability, correlation_id)?;
        let root = self.inspect_current_root(&record, correlation_id)?;
        if !within_record(&record, &root, published, relative_path) {
            return Err(ExternalGrantError::ScopeUnavailable);
        }
        Ok(record)
    }

    pub fn validate_create_parent(
        &self,
        grant_id: ExternalGrantId,
        session_id: SessionId,
        relative_path: &WorkspacePath,
        correlation_id: CorrelationId,
    ) -> Result<ValidatedExternalParent, ExternalGrantError> {
        let record = self.verify_active(
            grant_id,
            session_id,
            ExternalGrantCapability::Create,
            correlation_id,
        )?;
        if record.target_kind != ExternalGrantTargetKind::Directory
            || relative_path.is_root()
            || !within_authorized_depth(&record, relative_path)
        {
            return Err(ExternalGrantError::ScopeUnavailable);
        }
        let root = self.inspect_current_root(&record, correlation_id)?;
        let mut components = relative_path.as_str().split('/').collect::<Vec<_>>();
        let leaf = components.pop().ok_or(ExternalGrantError::ScopeUnavailable)?;
        let mut parent_path = PathBuf::from(&record.canonical_root.display);
        for component in components {
            parent_path.push(component);
        }
        let parent = open_verified_publish_parent(&parent_path)?;
        if !parent.info().is_directory || !final_path_within(&root, parent.info()) {
            return Err(ExternalGrantError::ScopeUnavailable);
        }
        let final_destination = parent.info().final_path.join(leaf);
        Ok(ValidatedExternalParent {
            grant: record,
            leaf_name: leaf.into(),
            parent_identity: domain_identity(&parent.info().identity),
            final_destination: canonical_path(final_destination.display().to_string()),
            parent,
        })
    }

    fn verify_active(
        &self,
        grant_id: ExternalGrantId,
        session_id: SessionId,
        capability: ExternalGrantCapability,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantRecord, ExternalGrantError> {
        let record = self.persistence.external_grant(grant_id)?;
        if record.session_id != session_id
            && !(record.term == ExternalGrantTerm::Workspace
                && record.workspace_id == self.persistence.workspace_of_session(session_id)?)
        {
            return Err(PersistenceError::ExternalGrantSessionMismatch.into());
        }
        if record.expires_at.is_some_and(|expiry| expiry <= UtcTimestamp::now()) {
            return Err(ExternalGrantError::Expired);
        }
        match record.lifecycle {
            ExternalGrantLifecycle::Active => {}
            ExternalGrantLifecycle::Suspended => return Err(ExternalGrantError::Suspended),
            ExternalGrantLifecycle::Revoked => return Err(ExternalGrantError::Revoked),
        }
        if !record.capabilities.contains(&capability) {
            return Err(ExternalGrantError::CapabilityUnavailable);
        }
        if !self.file_approval_in_force(&record)? {
            return Err(ExternalGrantError::Revoked);
        }
        self.inspect_current_root(&record, correlation_id)?;
        Ok(record)
    }

    fn inspect_current_root(
        &self,
        record: &ExternalGrantRecord,
        correlation_id: CorrelationId,
    ) -> Result<NativePathInfo, ExternalGrantError> {
        if let Some(permission_grant_id) = record.permission_grant_id {
            let permission = self.persistence.permission_grant(permission_grant_id)?;
            if self.ensure_reusable_workspace_permission(&permission, record.workspace_id).is_err()
            {
                let _ = self.persistence.revoke_external_grant(
                    record.id,
                    record.session_id,
                    correlation_id,
                );
                return Err(ExternalGrantError::PermissionGrantUnavailable);
            }
        }
        let info = inspect_existing_path(&PathBuf::from(&record.canonical_root.display), true)?;
        if domain_identity(&info.identity) != record.root_identity
            || info.is_directory != (record.target_kind == ExternalGrantTargetKind::Directory)
        {
            let _ = self.persistence.revoke_external_grant(
                record.id,
                record.session_id,
                correlation_id,
            );
            return Err(ExternalGrantError::IdentityChanged);
        }
        Ok(info)
    }

    fn revalidate_record(
        &self,
        mut record: ExternalGrantRecord,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantRecord, ExternalGrantError> {
        if record.lifecycle == ExternalGrantLifecycle::Revoked {
            return Err(ExternalGrantError::Revoked);
        }
        let selected = PathBuf::from(&record.selected_root.display);
        let info = inspect_existing_path(&selected, true)?;
        if domain_identity(&info.identity) != record.root_identity
            || info.is_directory != (record.target_kind == ExternalGrantTargetKind::Directory)
        {
            let _ = self.persistence.revoke_external_grant(
                record.id,
                record.session_id,
                correlation_id,
            );
            return Err(ExternalGrantError::IdentityChanged);
        }
        if let Some(permission_grant_id) = record.permission_grant_id {
            let permission = self.persistence.permission_grant(permission_grant_id)?;
            if self.ensure_reusable_workspace_permission(&permission, record.workspace_id).is_err()
            {
                let _ = self.persistence.revoke_external_grant(
                    record.id,
                    record.session_id,
                    correlation_id,
                );
                return Err(ExternalGrantError::PermissionGrantUnavailable);
            }
        }
        let expected = record.revalidation_revision;
        record.canonical_root = canonical_path(info.final_path.display().to_string());
        record.lifecycle = ExternalGrantLifecycle::Active;
        record.revalidation_revision =
            expected.checked_add(1).ok_or(ExternalGrantError::PermissionGrantUnavailable)?;
        record.updated_at = UtcTimestamp::now();
        record.suspended_at = None;
        self.persistence
            .replace_external_grant_after_revalidation(record, expected, correlation_id)
            .map_err(Into::into)
    }

    fn ensure_session_workspace(
        &self,
        session_id: SessionId,
        workspace_id: WorkspaceId,
    ) -> Result<(), ExternalGrantError> {
        let matches =
            self.persistence.workspace_session_projection()?.sessions.into_iter().any(|session| {
                session.session_id == session_id && session.workspace_id == workspace_id
            });
        if !matches {
            return Err(PersistenceError::ExternalGrantSessionMismatch.into());
        }
        Ok(())
    }

    fn ensure_reusable_workspace_permission(
        &self,
        permission: &PermissionGrant,
        workspace_id: WorkspaceId,
    ) -> Result<(), ExternalGrantError> {
        if permission.workspace_id != workspace_id
            || permission.lifetime != PermissionGrantLifetime::Workspace
            || permission.session_id.is_some()
            || permission.turn_task_id.is_some()
            || permission.revoked_at.is_some()
            || permission.consumed_at.is_some()
            || permission.expires_at.is_some_and(|expiry| expiry <= UtcTimestamp::now())
        {
            return Err(ExternalGrantError::PermissionGrantUnavailable);
        }
        Ok(())
    }
}

fn external_capabilities(permission: &PermissionGrant) -> Vec<ExternalGrantCapability> {
    let mut capabilities = Vec::new();
    for capability in &permission.capabilities {
        let mapped = match capability {
            ResolvedCapability::ExternalFileRead => Some(ExternalGrantCapability::Read),
            ResolvedCapability::ExternalFileWrite => Some(ExternalGrantCapability::Write),
            ResolvedCapability::ExternalFileCreate => Some(ExternalGrantCapability::Create),
            ResolvedCapability::ExternalFileDelete => Some(ExternalGrantCapability::Delete),
            _ => None,
        };
        if let Some(mapped) = mapped {
            capabilities.push(mapped);
        }
    }
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn target_path(
    record: &ExternalGrantRecord,
    relative_path: &WorkspacePath,
) -> Result<PathBuf, ExternalGrantError> {
    match record.target_kind {
        ExternalGrantTargetKind::File => {
            if !relative_path.is_root() {
                return Err(ExternalGrantError::ScopeUnavailable);
            }
            Ok(PathBuf::from(&record.canonical_root.display))
        }
        ExternalGrantTargetKind::Directory => {
            if !within_authorized_depth(record, relative_path) {
                return Err(ExternalGrantError::ScopeUnavailable);
            }
            let mut target = PathBuf::from(&record.canonical_root.display);
            if !relative_path.is_root() {
                for component in relative_path.as_str().split('/') {
                    target.push(component);
                }
            }
            Ok(target)
        }
    }
}

fn within_record(
    record: &ExternalGrantRecord,
    root: &NativePathInfo,
    candidate: &NativePathInfo,
    relative_path: &WorkspacePath,
) -> bool {
    match record.target_kind {
        ExternalGrantTargetKind::File => {
            relative_path.is_root() && candidate.identity == root.identity
        }
        ExternalGrantTargetKind::Directory => {
            final_path_within(root, candidate) && within_authorized_depth(record, relative_path)
        }
    }
}

/// The depth a grant authorizes, stated once for every check that needs it.
///
/// A file grant covers exactly its file. A directory grant covers the directory
/// itself and, when it is recursive, everything below it; a non-recursive directory
/// grant covers the directory and only its direct children — the next level down is a
/// deeper target and needs its own authorization. Classification, resolution,
/// creation and traversal all ask this function, so a path cannot be accepted by one
/// of them and refused by another, and a model's `recursive` request never widens it.
fn within_authorized_depth(record: &ExternalGrantRecord, relative_path: &WorkspacePath) -> bool {
    match record.target_kind {
        ExternalGrantTargetKind::File => relative_path.is_root(),
        ExternalGrantTargetKind::Directory => {
            record.recursive_scope || !relative_path.as_str().contains('/')
        }
    }
}

/// True when the grant authorizes the *contents* of this directory.
///
/// Naming a directory is not reading it. A non-recursive grant lets a direct child appear
/// as an object — `list` reports it, `find` matches its name — but its own entries are the
/// next level down and are not authorized. Enumeration therefore asks this question, not
/// [`within_authorized_depth`]: starting a walk at a direct child would otherwise read
/// grandchildren through the child's own name.
pub fn contents_are_authorized(
    record: &ExternalGrantRecord,
    relative_directory: &WorkspacePath,
) -> bool {
    match record.target_kind {
        ExternalGrantTargetKind::File => false,
        ExternalGrantTargetKind::Directory => {
            record.recursive_scope || relative_directory.is_root()
        }
    }
}

/// The facts a user approval freezes into a reusable directory grant.
#[derive(Clone)]
pub struct ApprovedDirectoryGrant {
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    pub canonical_root: CanonicalPath,
    pub root_identity: FileIdentity,
    pub recursive_scope: bool,
    pub capabilities: Vec<ExternalGrantCapability>,
    pub term: ExternalGrantTerm,
    pub turn_task_id: Option<TurnTaskId>,
    pub expires_at: Option<UtcTimestamp>,
}

/// Allow-once and deny authorize one operation or none. They do not mint a reusable grant.
#[must_use]
pub fn directory_grant_term(decision: ApprovalDecisionKind) -> Option<ExternalGrantTerm> {
    match decision {
        ApprovalDecisionKind::AllowSession => Some(ExternalGrantTerm::Session),
        ApprovalDecisionKind::AllowWorkspace => Some(ExternalGrantTerm::Workspace),
        ApprovalDecisionKind::AllowForCurrentTurn => Some(ExternalGrantTerm::Turn),
        ApprovalDecisionKind::AllowOnce | ApprovalDecisionKind::Deny => None,
    }
}

/// External file capabilities, sorted and unique, which is the form a grant stores.
#[must_use]
pub fn grant_capabilities(capabilities: &[ResolvedCapability]) -> Vec<ExternalGrantCapability> {
    let mut mapped = Vec::new();
    for capability in capabilities {
        let mapped_capability = match capability {
            ResolvedCapability::ExternalFileRead => Some(ExternalGrantCapability::Read),
            ResolvedCapability::ExternalFileWrite => Some(ExternalGrantCapability::Write),
            ResolvedCapability::ExternalFileCreate => Some(ExternalGrantCapability::Create),
            ResolvedCapability::ExternalFileDelete => Some(ExternalGrantCapability::Delete),
            _ => None,
        };
        if let Some(capability) = mapped_capability {
            mapped.push(capability);
        }
    }
    mapped.sort();
    mapped.dedup();
    mapped
}

fn grant_applies(
    record: &ExternalGrantRecord,
    session_id: SessionId,
    workspace_id: WorkspaceId,
    turn_task_id: Option<TurnTaskId>,
) -> bool {
    if record.lifecycle != ExternalGrantLifecycle::Active {
        return false;
    }
    if record.expires_at.is_some_and(|expiry| expiry <= UtcTimestamp::now()) {
        return false;
    }
    match record.term {
        ExternalGrantTerm::Session => record.session_id == session_id,
        ExternalGrantTerm::Workspace => record.workspace_id == workspace_id,
        ExternalGrantTerm::Turn => {
            record.session_id == session_id && record.turn_task_id == turn_task_id
        }
    }
}

fn canonical_path(display: String) -> CanonicalPath {
    CanonicalPath { utf16: display.encode_utf16().collect(), display }
}

/// The same mapping for a path that does not exist yet. It starts from the real
/// parent directory, which is why `.`/`..` never need to be resolved lexically here:
/// the parent is opened and verified, then only the last name is appended.
fn child_under(directory: &WorkspacePath, name: &str) -> Option<WorkspacePath> {
    if name.is_empty() {
        return None;
    }
    let text = if directory.is_root() { name.to_owned() } else { format!("{directory}/{name}") };
    WorkspacePath::parse(text).ok()
}

fn join_relative(components: std::path::Components<'_>) -> Result<WorkspacePath, ()> {
    let mut relative = String::new();
    for component in components {
        let text = component.as_os_str().to_string_lossy();
        if text.is_empty() || text == "." {
            continue;
        }
        if !relative.is_empty() {
            relative.push('/');
        }
        relative.push_str(&text);
    }
    let text = if relative.is_empty() { WorkspacePath::ROOT.to_owned() } else { relative };
    WorkspacePath::parse(text).map_err(|_| ())
}

#[cfg(test)]
mod finished_turn_list {
    use super::ExternalGrantService;
    use aura_agent_domain::{AgentTaskMode, TurnTask, TurnTaskState};
    use aura_domain_common::{
        AgentRunId, CorrelationId, EventId, ExternalGrantId, FileIdentity, Revision,
        RuntimeInstanceId, SessionId, TaskLineageId, TurnTaskId, UtcTimestamp, WorkspaceId,
    };
    use aura_permission_domain::{
        ExternalGrantAvailability, ExternalGrantCapability, ExternalGrantLifecycle,
        ExternalGrantRecord, ExternalGrantTargetKind, ExternalGrantTerm,
    };
    use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
    use aura_security_contract::CanonicalPath;
    use aura_workspace_domain::{CatalogFreshness, WorkspaceState, WorkspaceSummary};
    use std::sync::Arc;

    #[test]
    fn a_finished_turn_is_not_in_force_on_the_session_list() {
        let root = tempfile::tempdir().expect("root");
        let startup = PersistenceCoordinator::start(PersistenceOptions::new(
            root.path().join("data/aura.db"),
            root.path().join("data/artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence");
        let PersistenceStartup::Ready(persistence) = startup else {
            panic!("persistence requires recovery");
        };
        let workspace_id = WorkspaceId::new();
        let session_id = SessionId::new();
        let now = UtcTimestamp::now();
        persistence
            .upsert_workspace_session(
                WorkspaceSummary {
                    workspace_id,
                    display_name: "Grant".to_owned(),
                    selected_root: r"C:\Aura Tests\Grant".to_owned(),
                    canonical_root: r"C:\Aura Tests\Grant".to_owned(),
                    root_identity: None,
                    state: WorkspaceState::Online,
                    revision: Revision::INITIAL,
                    catalog_freshness: CatalogFreshness::Unavailable,
                    authorization_epoch: Revision::INITIAL,
                },
                aura_workspace_domain::SessionSummary {
                    session_id,
                    workspace_id,
                    title: "Grant".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("legacy session");
        let turn_task_id = TurnTaskId::new();
        let journal = Arc::new(
            crate::broker::BrokerJournal::open(&root.path().join("journal")).expect("journal"),
        );
        {
            let mut store = journal.lock().expect("journal lock");
            store
                .database
                .unit_of_work(|tx| -> aura_persistence_core::StoreResult<()> {
                    aura_persistence_v2::workspace::insert(
                        tx,
                        &WorkspaceSummary {
                            workspace_id,
                            display_name: "Grant".to_owned(),
                            selected_root: r"C:\Aura Tests\Grant".to_owned(),
                            canonical_root: r"C:\Aura Tests\Grant".to_owned(),
                            root_identity: None,
                            state: WorkspaceState::Offline,
                            revision: Revision::INITIAL,
                            catalog_freshness: CatalogFreshness::Unavailable,
                            authorization_epoch: Revision::INITIAL,
                        },
                    )?;
                    aura_persistence_v2::workspace::insert_session(
                        tx,
                        &aura_persistence_v2::workspace::SessionMetadata {
                            id: session_id,
                            workspace_id,
                            title: "Grant".to_owned(),
                            created_at: now,
                            updated_at: now,
                        },
                    )?;
                    let turn = TurnTask {
                        turn_task_id,
                        session_id,
                        workspace_id,
                        root_user_event_id: EventId::new(),
                        lineage_id: TaskLineageId::new(),
                        lineage_resolution_required: false,
                        agent_run_id: AgentRunId::new(),
                        mode: AgentTaskMode::Normal,
                        state: TurnTaskState::Completed { final_report: "done".to_owned() },
                        revision: Revision::INITIAL,
                        created_at: now,
                        updated_at: now,
                    };
                    tx.execute(
                        "INSERT INTO turn_tasks VALUES(?1,?2,'completed',1,?3)",
                        rusqlite::params![
                            turn_task_id.to_string(),
                            session_id.to_string(),
                            aura_persistence_core::record::encode(&turn)?
                        ],
                    )?;
                    Ok(())
                })
                .expect("finished turn");
        }
        let path = r"C:\Aura Tests\Grant\file.txt";
        let record = ExternalGrantRecord {
            id: ExternalGrantId::new(),
            session_id,
            workspace_id,
            permission_grant_id: None,
            selected_root: CanonicalPath {
                display: path.to_owned(),
                utf16: path.encode_utf16().collect(),
            },
            canonical_root: CanonicalPath {
                display: path.to_owned(),
                utf16: path.encode_utf16().collect(),
            },
            root_identity: FileIdentity {
                volume_serial_number: "1".to_owned(),
                file_id: "ab".repeat(16),
            },
            target_kind: ExternalGrantTargetKind::File,
            capabilities: vec![ExternalGrantCapability::Read],
            recursive_scope: false,
            lifecycle: ExternalGrantLifecycle::Active,
            revalidation_revision: 1,
            created_at: now,
            updated_at: now,
            suspended_at: None,
            revoked_at: None,
            expires_at: None,
            turn_task_id: Some(turn_task_id),
            term: ExternalGrantTerm::Turn,
        };
        persistence.create_external_grant(record.clone(), CorrelationId::new()).expect("grant");
        let service = ExternalGrantService::new(Arc::new(persistence));
        service.bind_turn_journal(journal);
        let listed = service.list_session(session_id).expect("list");
        let view = listed.iter().find(|item| item.id == record.id).expect("the turn grant");
        assert_ne!(view.availability, ExternalGrantAvailability::InForce);
        assert_eq!(view.availability, ExternalGrantAvailability::TurnEnded);
    }
}
