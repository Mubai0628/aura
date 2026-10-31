use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{ErrorCategory, ErrorCode};
use aura_persistence::{
    ExactToolDefinitionDescriptionV1, PersistenceCoordinator, PersistenceError,
    ToolProjectionDraft, ToolRegistryBuildCommitV1, ToolRegistryBuildRequestV1,
};
use aura_projection_contract::ProjectionChange;
use aura_tool_contract::{
    ToolDefinitionSnapshotRef, ToolLoadRequestV1, ToolLoadResultV1, ToolLoadStateV1,
    ToolRegistryErrorCode, ToolRegistryScopeV1, ToolRegistrySnapshotRef, ToolSearchPageV1,
    ToolSearchRequestV1, ToolSettingsSnapshotV1,
};
use aura_tool_projection::{ToolProjectionErrorView, ToolSettingsProjection};
use aura_tool_registry::{ToolSearchError, ToolSearchServiceV1};
use thiserror::Error;

/// Core-owned entrypoint for the frozen Registry control operations. The
/// caller must supply an exact `trs_` and, for mutations, a trusted Registry
/// build context assembled by Core. None of these methods select a newer
/// Snapshot or accept Renderer/model supplied eligibility facts.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolRegistryControlServiceV1;

/// The Session a scope belongs to, when it has one.
fn scope_session(scope: &ToolRegistryScopeV1) -> Option<aura_domain_common::SessionId> {
    match scope {
        ToolRegistryScopeV1::Workspace { .. } => None,
        ToolRegistryScopeV1::Session { session_id, .. }
        | ToolRegistryScopeV1::ChildAgent { session_id, .. } => Some(*session_id),
    }
}

pub struct PublishedToolRegistryBuildV1 {
    pub commit: ToolRegistryBuildCommitV1,
    pub projection_publish_error: Option<PersistenceError>,
}

pub(crate) struct PublishedToolRegistryMutationV1 {
    pub result: ToolLoadResultV1,
    pub projection_publish_error: Option<PersistenceError>,
}

pub struct PublishedToolSettingsProjectionV1 {
    pub change: ProjectionChange,
    pub projection_publish_error: Option<PersistenceError>,
}

/// What one change-driven Registry refresh did.
pub struct ToolRegistryRefreshReportV1 {
    /// Scopes whose head was rebuilt and published.
    pub refreshed: Vec<ToolRegistryScopeV1>,
    /// Scopes that this change reached but that have no work left: their work
    /// already ended, or the scope no longer exists. Their snapshots are immutable
    /// and are deliberately not rewritten.
    pub untouched: usize,
    /// Current scopes this change cannot have affected at all, so their heads were
    /// not rebuilt. Kept apart from `untouched`: "this change reached nothing here"
    /// and "this work already ended" are different facts, and only the first one
    /// shows that an unrelated active scope kept its exact snapshot.
    pub skipped_unaffected: usize,
    /// Scopes that should have been refreshed but failed, with the stage that
    /// failed. A failure is reported to the caller instead of being swallowed,
    /// because a stale head decides what the next ModelStep may use.
    pub failures: Vec<ToolRegistryRefreshFailureV1>,
}

/// One scope that could not be refreshed, and where it stopped.
pub struct ToolRegistryRefreshFailureV1 {
    pub scope: ToolRegistryScopeV1,
    /// `scope_state`, `permission`, `permission_context_hash`, `corpus`, `build` or
    /// `publish`. Named so a partial refresh is diagnosable without a second run.
    pub stage: &'static str,
    pub reason: String,
}

impl ToolRegistryRefreshReportV1 {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Which scopes a committed change can still affect.
///
/// A change to what is installed, trusted, enabled or configured is visible to
/// every scope, so those scopes are all rebuilt. A pin is narrower: a Workspace pin
/// is part of the Workspace authority that its Sessions and Child Agents layer on
/// top of, and a Session pin is part of that Session's authority and of the Child
/// Agents below it. Nothing else is rebuilt, so an unrelated active scope keeps the
/// exact snapshot its running work was planned against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolRegistryChangeImpactV1 {
    /// Installed / trusted / enabled / configured facts: any current scope can see
    /// them.
    EveryCurrentScope,
    /// A Workspace pin: that Workspace, its Sessions and their Child Agents.
    Workspace(aura_domain_common::WorkspaceId),
    /// A Session pin: that Session and its Child Agents. Other Sessions of the same
    /// Workspace keep their own authority.
    Session(aura_domain_common::SessionId),
}

impl ToolRegistryChangeImpactV1 {
    #[must_use]
    pub(crate) fn affects(&self, scope: &ToolRegistryScopeV1) -> bool {
        match (self, scope) {
            (Self::EveryCurrentScope, _) => true,
            (
                Self::Workspace(workspace),
                ToolRegistryScopeV1::Workspace { workspace_id }
                | ToolRegistryScopeV1::Session { workspace_id, .. }
                | ToolRegistryScopeV1::ChildAgent { workspace_id, .. },
            ) => workspace == workspace_id,
            (
                Self::Session(session),
                ToolRegistryScopeV1::Session { session_id, .. }
                | ToolRegistryScopeV1::ChildAgent { session_id, .. },
            ) => session == session_id,
            (Self::Session(_), ToolRegistryScopeV1::Workspace { .. }) => false,
        }
    }
}

impl ToolRegistryControlServiceV1 {
    pub fn publish_settings(
        &self,
        persistence: &PersistenceCoordinator,
        settings: ToolSettingsSnapshotV1,
        configuration_rejected: bool,
    ) -> Result<PublishedToolSettingsProjectionV1, ToolRegistryControlErrorV1> {
        settings.validate().map_err(|_| ToolRegistryControlErrorV1::SettingsInvalid)?;
        let configuration_issue = configuration_rejected.then(settings_invalid_projection_error);
        let mut changes = persistence.commit_tool_projection_snapshots(vec![
            ToolProjectionDraft::ToolSettings(ToolSettingsProjection {
                settings,
                configuration_issue,
            }),
        ])?;
        let change = changes.pop().ok_or(ToolRegistryControlErrorV1::ProjectionMissing)?;
        if !changes.is_empty() {
            return Err(ToolRegistryControlErrorV1::ProjectionMultiplicity);
        }
        let projection_publish_error =
            persistence.publish_tool_projection_changes(std::slice::from_ref(&change)).err();
        Ok(PublishedToolSettingsProjectionV1 { change, projection_publish_error })
    }

    pub fn build_scope(
        &self,
        persistence: &PersistenceCoordinator,
        request: ToolRegistryBuildRequestV1,
    ) -> Result<PublishedToolRegistryBuildV1, ToolRegistryControlErrorV1> {
        let commit = persistence.build_and_commit_tool_registry_snapshot(request)?;
        let projection_publish_error =
            persistence.publish_tool_projection_changes(&commit.projection_changes).err();
        Ok(PublishedToolRegistryBuildV1 { commit, projection_publish_error })
    }

    /// Rebuilds the Registry head of every current scope that a committed change
    /// affects, after install, trust, enable, configuration or pin.
    ///
    /// Only the *current* scopes are refreshed: a Workspace scope always is, a
    /// Session/ChildAgent scope only while its TurnTask and AgentRun have not reached
    /// a terminal state. Snapshots that were already issued stay immutable — they are
    /// what a running ModelStep was planned against — and no scope is invented for a
    /// display that has no execution context. `impact` narrows the walk further, so a
    /// pin in one Session cannot rewrite another Session's head.
    ///
    /// Per-scope failures are collected, with the stage that failed, and returned
    /// rather than swallowed: a head that could not be rebuilt would otherwise let
    /// the next step plan against a stale Registry, and one bad scope must not stop
    /// the refresh of the others.
    pub fn refresh_current_scopes(
        &self,
        persistence: &PersistenceCoordinator,
        settings: ToolSettingsSnapshotV1,
        impact: ToolRegistryChangeImpactV1,
        correlation_id: aura_domain_common::CorrelationId,
    ) -> Result<ToolRegistryRefreshReportV1, ToolRegistryControlErrorV1> {
        let heads = persistence.list_tool_registry_scope_heads()?;
        let mut report = ToolRegistryRefreshReportV1 {
            refreshed: Vec::new(),
            untouched: 0,
            skipped_unaffected: 0,
            failures: Vec::new(),
        };
        for (scope, head) in heads {
            if !impact.affects(&scope) {
                report.skipped_unaffected += 1;
                continue;
            }
            match self.refresh_scope_head(persistence, &scope, &head, &settings, correlation_id) {
                Ok(true) => report.refreshed.push(scope),
                Ok(false) => report.untouched += 1,
                Err(failure) => report.failures.push(*failure),
            }
        }
        Ok(report)
    }

    /// Rebuilds one scope's head, reporting where it stopped instead of ending the
    /// caller's loop.
    fn refresh_scope_head(
        &self,
        persistence: &PersistenceCoordinator,
        scope: &ToolRegistryScopeV1,
        head: &ToolRegistrySnapshotRef,
        settings: &ToolSettingsSnapshotV1,
        correlation_id: aura_domain_common::CorrelationId,
    ) -> Result<bool, Box<ToolRegistryRefreshFailureV1>> {
        let failed = |stage: &'static str, reason: String| {
            Box::new(ToolRegistryRefreshFailureV1 { scope: scope.clone(), stage, reason })
        };
        let current = persistence
            .tool_registry_scope_is_current(scope)
            .map_err(|error| failed("scope_state", error.to_string()))?;
        if !current {
            return Ok(false);
        }
        let permission = scope_session(scope)
            .map(|session| persistence.session_permission_state(session))
            .transpose()
            .map_err(|error| failed("permission", error.to_string()))?;
        let mode_permission_context_hash = canonical_sha256(&permission).map_err(|_| {
            failed(
                "permission_context_hash",
                "the Session permission context is not hashable".into(),
            )
        })?;
        let core_exposed_tool_ids = persistence
            .read_tool_registry_search_corpus(head)
            .map_err(|error| failed("corpus", error.to_string()))?
            .map(|corpus| {
                corpus
                    .snapshot
                    .catalog_entries
                    .iter()
                    .filter(|entry| entry.load_state == ToolLoadStateV1::Core)
                    .map(|entry| entry.tool_id)
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        let request = ToolRegistryBuildRequestV1 {
            scope: scope.clone(),
            settings: settings.clone(),
            mode_permission_context_hash,
            core_exposed_tool_ids,
            non_package_candidates: Vec::new(),
            created_at: aura_domain_common::UtcTimestamp::now(),
            correlation_id,
        };
        match self.build_scope(persistence, request) {
            Ok(published) => match published.projection_publish_error {
                Some(error) => Err(failed(
                    "publish",
                    format!("Registry head rebuilt but not published: {error}"),
                )),
                None => Ok(true),
            },
            Err(error) => Err(failed("build", error.to_string())),
        }
    }

    pub(crate) fn search(
        &self,
        persistence: &PersistenceCoordinator,
        current_snapshot_ref: &ToolRegistrySnapshotRef,
        audience_visibility_fingerprint: &CanonicalHashV1,
        request: &ToolSearchRequestV1,
    ) -> Result<ToolSearchPageV1, ToolRegistryControlErrorV1> {
        ToolSearchServiceV1::new(persistence, persistence)
            .search(current_snapshot_ref, audience_visibility_fingerprint, request)
            .map_err(ToolRegistryControlErrorV1::Search)
    }

    pub(crate) fn describe(
        &self,
        persistence: &PersistenceCoordinator,
        definition_ref: &ToolDefinitionSnapshotRef,
    ) -> Result<ExactToolDefinitionDescriptionV1, ToolRegistryControlErrorV1> {
        persistence
            .describe_tool_definition_exact(definition_ref)?
            .ok_or(ToolRegistryControlErrorV1::DefinitionUnavailable)
    }

    pub fn list_loaded(
        &self,
        persistence: &PersistenceCoordinator,
        snapshot_ref: &ToolRegistrySnapshotRef,
    ) -> Result<Vec<ToolDefinitionSnapshotRef>, ToolRegistryControlErrorV1> {
        persistence
            .list_loaded_tool_definitions_exact(snapshot_ref)?
            .ok_or(ToolRegistryControlErrorV1::SnapshotUnavailable)
    }

    pub(crate) fn load(
        &self,
        persistence: &PersistenceCoordinator,
        request: ToolLoadRequestV1,
        registry_request: ToolRegistryBuildRequestV1,
    ) -> Result<PublishedToolRegistryMutationV1, ToolRegistryControlErrorV1> {
        let commit =
            persistence.load_tools_ephemerally_with_projections(request, registry_request)?;
        let projection_publish_error =
            persistence.publish_tool_projection_changes(&commit.projection_changes).err();
        Ok(PublishedToolRegistryMutationV1 { result: commit.result, projection_publish_error })
    }

    pub(crate) fn unload(
        &self,
        persistence: &PersistenceCoordinator,
        request: ToolLoadRequestV1,
        registry_request: ToolRegistryBuildRequestV1,
    ) -> Result<PublishedToolRegistryMutationV1, ToolRegistryControlErrorV1> {
        let commit =
            persistence.unload_tools_ephemerally_with_projections(request, registry_request)?;
        let projection_publish_error =
            persistence.publish_tool_projection_changes(&commit.projection_changes).err();
        Ok(PublishedToolRegistryMutationV1 { result: commit.result, projection_publish_error })
    }

    /// Removes only the exact TurnTask ephemeral loads recorded by Core and
    /// represented as ephemeral in the caller-supplied Registry Snapshot.
    /// Pinned/Core tools are never inferred or removed, and this path never
    /// selects a newer scope head on behalf of the caller.
    pub(crate) fn cleanup_turn_task_ephemeral_loads(
        &self,
        persistence: &PersistenceCoordinator,
        source_snapshot_ref: ToolRegistrySnapshotRef,
        registry_request: ToolRegistryBuildRequestV1,
    ) -> Result<PublishedToolRegistryMutationV1, ToolRegistryControlErrorV1> {
        let ToolRegistryScopeV1::Session { workspace_id, session_id, turn_task_id, .. } =
            &registry_request.scope
        else {
            return Err(ToolRegistryControlErrorV1::CleanupScopeInvalid);
        };
        let source = persistence
            .read_tool_registry_search_corpus(&source_snapshot_ref)?
            .ok_or(ToolRegistryControlErrorV1::SnapshotUnavailable)?;
        if source.snapshot.scope != registry_request.scope {
            return Err(ToolRegistryControlErrorV1::SnapshotScopeMismatch);
        }

        let authority = persistence.tool_authority_snapshot(
            Some(*workspace_id),
            Some(*session_id),
            Some(*turn_task_id),
        )?;
        let mut definitions = authority
            .ephemeral_loads
            .into_iter()
            .map(|load| ToolDefinitionSnapshotRef {
                tool_id: load.tool_id,
                tool_version: load.tool_version,
                registry_snapshot_ref: source_snapshot_ref.clone(),
            })
            .collect::<Vec<_>>();
        definitions.sort_by_key(|definition| definition.tool_id);

        if definitions.iter().any(|definition| {
            !source.snapshot.catalog_entries.iter().any(|entry| {
                entry.tool_id == definition.tool_id
                    && entry.tool_version == definition.tool_version
                    && entry.load_state == ToolLoadStateV1::Ephemeral
            })
        }) {
            return Err(ToolRegistryControlErrorV1::EphemeralAuthorityMismatch);
        }
        if definitions.is_empty() {
            return Ok(PublishedToolRegistryMutationV1 {
                result: ToolLoadResultV1 {
                    changed: false,
                    resulting_registry_snapshot_ref: source_snapshot_ref,
                },
                projection_publish_error: None,
            });
        }

        let expected_snapshot_ref = source_snapshot_ref;
        let published =
            self.unload(persistence, ToolLoadRequestV1 { definitions }, registry_request)?;
        if !published.result.changed
            && published.result.resulting_registry_snapshot_ref != expected_snapshot_ref
        {
            return Err(ToolRegistryControlErrorV1::SnapshotAdvancedDuringCleanup);
        }
        Ok(published)
    }
}

#[derive(Debug, Error)]
pub enum ToolRegistryControlErrorV1 {
    #[error("the Built-in Tool request is invalid")]
    RequestInvalid,
    #[error("the Built-in Tool response could not be encoded")]
    ResponseInvalid,
    #[error("a Core-owned Registry rebuild context is required for this operation")]
    BuildContextMissing,
    #[error("the Tool Settings Snapshot is invalid")]
    SettingsInvalid,
    #[error("the Tool Settings projection transaction returned no change")]
    ProjectionMissing,
    #[error("the Tool Settings projection transaction returned multiple changes")]
    ProjectionMultiplicity,
    #[error("the exact Registry Snapshot is unavailable")]
    SnapshotUnavailable,
    #[error("the exact Tool definition is unavailable in the requested Registry Snapshot")]
    DefinitionUnavailable,
    #[error("TurnTask ephemeral cleanup requires a Session Registry scope")]
    CleanupScopeInvalid,
    #[error("the exact Registry Snapshot scope does not match the cleanup request")]
    SnapshotScopeMismatch,
    #[error("TurnTask ephemeral authority does not match the exact Registry Snapshot")]
    EphemeralAuthorityMismatch,
    #[error("the Registry scope advanced while TurnTask ephemeral cleanup was pending")]
    SnapshotAdvancedDuringCleanup,
    #[error(transparent)]
    Search(#[from] ToolSearchError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

fn settings_invalid_projection_error() -> ToolProjectionErrorView {
    ToolProjectionErrorView {
        code: ErrorCode::new(ToolRegistryErrorCode::SettingsInvalid.as_str())
            .expect("registered Tool Registry code"),
        category: ErrorCategory::Conflict,
        message: "工具设置无效；Aura 已保留最近一次有效配置。".to_owned(),
        retryable: false,
    }
}
