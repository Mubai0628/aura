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
pub(crate) struct ToolRegistryControlServiceV1;

/// The Session a scope belongs to, when it has one.
fn scope_session(scope: &ToolRegistryScopeV1) -> Option<aura_domain_common::SessionId> {
    match scope {
        ToolRegistryScopeV1::Workspace { .. } => None,
        ToolRegistryScopeV1::Session { session_id, .. }
        | ToolRegistryScopeV1::ChildAgent { session_id, .. } => Some(*session_id),
    }
}

pub(crate) struct PublishedToolRegistryBuildV1 {
    pub commit: ToolRegistryBuildCommitV1,
    pub projection_publish_error: Option<PersistenceError>,
}

pub(crate) struct PublishedToolRegistryMutationV1 {
    pub result: ToolLoadResultV1,
    pub projection_publish_error: Option<PersistenceError>,
}

pub(crate) struct PublishedToolSettingsProjectionV1 {
    pub change: ProjectionChange,
    pub projection_publish_error: Option<PersistenceError>,
}

/// What one change-driven Registry refresh did.
pub(crate) struct ToolRegistryRefreshReportV1 {
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
pub(crate) struct ToolRegistryRefreshFailureV1 {
    pub scope: ToolRegistryScopeV1,
    /// `scope_state`, `permission`, `permission_context_hash`, `corpus`, `build` or
    /// `publish`. Named so a partial refresh is diagnosable without a second run.
    pub stage: &'static str,
    pub reason: String,
}

impl ToolRegistryRefreshReportV1 {
    #[must_use]
    pub(crate) fn is_clean(&self) -> bool {
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
pub(crate) enum ToolRegistryChangeImpactV1 {
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
    pub(crate) fn publish_settings(
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

    pub(crate) fn build_scope(
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
    pub(crate) fn refresh_current_scopes(
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

    pub(crate) fn list_loaded(
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
pub(crate) enum ToolRegistryControlErrorV1 {
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use aura_domain_common::{CorrelationId, RuntimeInstanceId};
    use aura_persistence::{PersistenceOptions, PersistenceStartup};
    use aura_projection_contract::{ProjectionKind, ProjectionScope};

    use super::*;

    #[test]
    fn settings_projection_publishes_lkg_and_a_safe_rejection_issue() {
        let root = tempfile::tempdir().expect("temporary data root");
        let PersistenceStartup::Ready(persistence) =
            PersistenceCoordinator::start(PersistenceOptions::new(
                root.path().join("aura.db"),
                root.path().join("artifacts"),
                RuntimeInstanceId::new(),
            ))
            .expect("persistence startup")
        else {
            panic!("fixture persistence must be ready")
        };
        let updates = persistence.subscribe_tool_projections().expect("projection subscription");
        let settings = ToolSettingsSnapshotV1::default();
        let service = ToolRegistryControlServiceV1;

        let initial = service
            .publish_settings(&persistence, settings.clone(), false)
            .expect("initial settings projection");
        assert!(initial.projection_publish_error.is_none());
        assert!(matches!(
            updates.recv_timeout(Duration::from_secs(1)).expect("published initial projection"),
            ProjectionChange::ToolSettings { payload, .. }
                if payload.settings == settings && payload.configuration_issue.is_none()
        ));

        let rejected = service
            .publish_settings(&persistence, settings.clone(), true)
            .expect("rejected settings projection");
        assert!(rejected.projection_publish_error.is_none());
        let latest = persistence
            .latest_tool_projection(ProjectionKind::ToolSettings, ProjectionScope::Global)
            .expect("latest settings projection")
            .expect("persisted settings projection");
        assert!(matches!(
            latest,
            ProjectionChange::ToolSettings { payload, .. }
                if payload.settings == settings
                    && payload.configuration_issue.as_ref().is_some_and(|issue| {
                        issue.code.as_str() == ToolRegistryErrorCode::SettingsInvalid.as_str()
                            && issue.category == ErrorCategory::Conflict
                            && !issue.retryable
                    })
        ));
    }

    /// Seeds one workspace with two sessions: one whose TurnTask and AgentRun are
    /// still running, one whose work already ended. Both get a Registry head.
    fn refresh_fixture(
        root: &std::path::Path,
    ) -> (PersistenceCoordinator, ToolRegistryScopeV1, ToolRegistryScopeV1, ToolRegistryScopeV1)
    {
        let (persistence, database_path, mut scopes) = running_sessions_fixture(root, 2);
        let ended_scope = scopes.pop().expect("the second Session scope");
        let running_scope = scopes.pop().expect("the first Session scope");
        let workspace_scope = scopes.pop().expect("the workspace scope");
        let ToolRegistryScopeV1::Session { turn_task_id, agent_run_id, .. } = &ended_scope else {
            panic!("the second scope has to be a Session scope")
        };
        let connection = rusqlite::Connection::open(&database_path).expect("open fixture database");
        connection
            .execute(
                "UPDATE turn_tasks SET state='completed' WHERE turn_task_id=?1",
                [turn_task_id.to_string()],
            )
            .expect("end the second TurnTask");
        connection
            .execute(
                "UPDATE agent_runs SET state='completed' WHERE agent_run_id=?1",
                [agent_run_id.to_string()],
            )
            .expect("end the second AgentRun");
        drop(connection);
        (persistence, workspace_scope, running_scope, ended_scope)
    }

    /// One workspace with `count` running Sessions, each with its own committed head.
    /// The first entry of the returned vector is the workspace scope.
    fn running_sessions_fixture(
        root: &std::path::Path,
        count: usize,
    ) -> (PersistenceCoordinator, std::path::PathBuf, Vec<ToolRegistryScopeV1>) {
        let database_path = root.join("aura.db");
        let PersistenceStartup::Ready(persistence) =
            PersistenceCoordinator::start(PersistenceOptions::new(
                database_path.clone(),
                root.join("artifacts"),
                RuntimeInstanceId::new(),
            ))
            .expect("persistence startup")
        else {
            panic!("fixture persistence must be ready")
        };
        let workspace_id = aura_domain_common::WorkspaceId::new();
        let mut scopes = vec![ToolRegistryScopeV1::Workspace { workspace_id }];
        for _ in 0..count {
            let session_id = aura_domain_common::SessionId::new();
            let turn_task_id = aura_domain_common::TurnTaskId::new();
            let agent_run_id = aura_domain_common::AgentRunId::new();
            crate::p07_test_support::seed_session_scope(
                &persistence,
                &database_path,
                workspace_id,
                session_id,
                turn_task_id,
                agent_run_id,
                root,
            )
            .expect("seed session scope");
            scopes.push(ToolRegistryScopeV1::Session {
                workspace_id,
                session_id,
                turn_task_id,
                agent_run_id,
            });
        }
        for scope in &scopes {
            ToolRegistryControlServiceV1
                .build_scope(&persistence, crate::p07_test_support::registry_request(scope.clone()))
                .expect("initial Registry head");
        }
        (persistence, database_path, scopes)
    }

    fn head(
        persistence: &PersistenceCoordinator,
        scope: &ToolRegistryScopeV1,
    ) -> ToolRegistrySnapshotRef {
        persistence
            .read_tool_registry_scope_head(scope)
            .expect("read the head")
            .expect("the scope has a head")
    }

    fn refresh(
        persistence: &PersistenceCoordinator,
        impact: ToolRegistryChangeImpactV1,
    ) -> ToolRegistryRefreshReportV1 {
        ToolRegistryControlServiceV1
            .refresh_current_scopes(
                persistence,
                ToolSettingsSnapshotV1::default(),
                impact,
                CorrelationId::new(),
            )
            .expect("refresh the current scopes")
    }

    fn failure_reasons(report: &ToolRegistryRefreshReportV1) -> Vec<&str> {
        report.failures.iter().map(|failure| failure.reason.as_str()).collect()
    }

    /// Writes one invalid Session pin row, so that only this Session's rebuild fails.
    fn corrupt_session_pin(
        database_path: &std::path::Path,
        session_id: aura_domain_common::SessionId,
    ) {
        let connection = rusqlite::Connection::open(database_path).expect("open fixture database");
        connection
            .execute(
                "INSERT INTO session_tool_pins(
                     session_id, workspace_id, tool_id, tool_version_json, entity_revision,
                     record_json, record_hash_json, updated_at
                 ) SELECT ?1, workspace_id, 'tpk_injected_fault', '1.0.0', 1,
                          '{\"not\":\"a pin\"}', '\"sha256:0\"', updated_at
                   FROM session_permission_states WHERE session_id=?1",
                [session_id.to_string()],
            )
            .expect("inject the invalid pin row");
    }

    fn repair_session_pins(
        database_path: &std::path::Path,
        session_id: aura_domain_common::SessionId,
    ) {
        let connection = rusqlite::Connection::open(database_path).expect("open fixture database");
        connection
            .execute("DELETE FROM session_tool_pins WHERE session_id=?1", [session_id.to_string()])
            .expect("remove the injected pin row");
    }

    #[test]
    fn a_change_refreshes_current_scopes_and_never_rewrites_an_ended_one() {
        let root = tempfile::tempdir().expect("temporary data root");
        let (persistence, workspace_scope, running_scope, ended_scope) =
            refresh_fixture(root.path());
        let before_workspace = head(&persistence, &workspace_scope);
        let before_running = head(&persistence, &running_scope);
        let before_ended = head(&persistence, &ended_scope);

        let report = refresh(&persistence, ToolRegistryChangeImpactV1::EveryCurrentScope);
        assert!(report.is_clean(), "{:?}", failure_reasons(&report));
        assert_eq!(report.refreshed.len(), 2, "workspace and the running session only");
        assert_eq!(report.untouched, 1, "the ended TurnTask keeps its head");
        assert_eq!(report.skipped_unaffected, 0, "a global change reaches every current scope");

        let after_workspace = head(&persistence, &workspace_scope);
        let after_running = head(&persistence, &running_scope);
        let after_ended = head(&persistence, &ended_scope);
        assert_ne!(
            after_workspace.tool_registry_snapshot_id, before_workspace.tool_registry_snapshot_id,
            "the workspace head must serve the new facts"
        );
        assert_ne!(
            after_running.tool_registry_snapshot_id, before_running.tool_registry_snapshot_id,
            "the running session head must serve the new facts"
        );
        assert_eq!(
            after_ended, before_ended,
            "an ended scope keeps its immutable snapshot: it is what running work was planned against"
        );
        // The superseded snapshots are still readable — nothing was rewritten in place.
        for superseded in [before_workspace, before_running, before_ended] {
            assert!(
                persistence
                    .read_tool_registry_search_corpus(&superseded)
                    .expect("read superseded snapshot")
                    .is_some(),
                "a superseded Registry snapshot must stay readable"
            );
        }
    }

    #[test]
    fn a_session_pin_refreshes_only_its_own_session() {
        // Cross-Session isolation: two Sessions run in the same Workspace with their
        // own heads. A pin committed in the first must reach that Session and leave the
        // second Session's head — and the Workspace scope — exactly as they were.
        let root = tempfile::tempdir().expect("temporary data root");
        let (persistence, _database_path, scopes) = running_sessions_fixture(root.path(), 2);
        let workspace_scope = scopes[0].clone();
        let first = scopes[1].clone();
        let second = scopes[2].clone();
        let ToolRegistryScopeV1::Session { session_id: first_session, .. } = first else {
            panic!("the second scope has to be a Session scope")
        };

        let before_workspace = head(&persistence, &workspace_scope);
        let before_first = head(&persistence, &first);
        let before_second = head(&persistence, &second);

        let report = refresh(&persistence, ToolRegistryChangeImpactV1::Session(first_session));
        assert!(report.is_clean(), "{:?}", failure_reasons(&report));
        assert_eq!(report.refreshed, vec![first.clone()], "only the pinned Session is rebuilt");
        assert_eq!(
            report.skipped_unaffected, 2,
            "the Workspace and the other Session keep their heads"
        );
        assert_eq!(report.untouched, 0);

        assert_ne!(
            head(&persistence, &first).tool_registry_snapshot_id,
            before_first.tool_registry_snapshot_id,
            "the pinned Session must serve a head built from the new facts"
        );
        assert_eq!(
            head(&persistence, &second),
            before_second,
            "another active Session keeps the exact snapshot its running work was planned against"
        );
        assert_eq!(
            head(&persistence, &workspace_scope),
            before_workspace,
            "a Session pin is not a Workspace fact"
        );
    }

    #[test]
    fn a_workspace_pin_reaches_its_sessions_and_no_other_workspace() {
        let root = tempfile::tempdir().expect("temporary data root");
        let first_root = root.path().join("first");
        let second_root = root.path().join("second");
        std::fs::create_dir_all(&first_root).expect("first workspace root");
        std::fs::create_dir_all(&second_root).expect("second workspace root");
        let (first_persistence, _first_database, first_scopes) =
            running_sessions_fixture(&first_root, 1);
        let (second_persistence, _second_database, second_scopes) =
            running_sessions_fixture(&second_root, 1);
        let ToolRegistryScopeV1::Workspace { workspace_id } = first_scopes[0] else {
            panic!("the first scope has to be a Workspace scope")
        };

        let before_second_workspace = head(&second_persistence, &second_scopes[0]);
        let before_second_session = head(&second_persistence, &second_scopes[1]);

        let report =
            refresh(&first_persistence, ToolRegistryChangeImpactV1::Workspace(workspace_id));
        assert!(report.is_clean(), "{:?}", failure_reasons(&report));
        // Enumeration order is by scope hash, so the rebuilt set is compared as a set.
        let mut refreshed = report.refreshed.clone();
        refreshed.sort_by_key(|scope| format!("{scope:?}"));
        let mut expected = vec![first_scopes[0].clone(), first_scopes[1].clone()];
        expected.sort_by_key(|scope| format!("{scope:?}"));
        assert_eq!(refreshed, expected, "the Workspace and its own Session are rebuilt");
        assert_eq!(report.skipped_unaffected, 0);
        assert_eq!(
            head(&second_persistence, &second_scopes[0]),
            before_second_workspace,
            "another Workspace keeps its head"
        );
        assert_eq!(
            head(&second_persistence, &second_scopes[1]),
            before_second_session,
            "another Workspace's Session keeps its head"
        );
    }

    #[test]
    fn a_session_that_cannot_be_rebuilt_is_reported_and_does_not_stop_the_others() {
        // One bad scope must not end the walk: the other scopes still get their new
        // heads, the failure names the stage, and a later legitimate change recovers the
        // scope that failed — a stale head can never pass as a successful refresh.
        let root = tempfile::tempdir().expect("temporary data root");
        let (persistence, database_path, scopes) = running_sessions_fixture(root.path(), 2);
        let first = scopes[1].clone();
        let second = scopes[2].clone();
        let ToolRegistryScopeV1::Session { session_id: first_session, .. } = &first else {
            panic!("the second scope has to be a Session scope")
        };
        let before_first = head(&persistence, &first);
        let before_second = head(&persistence, &second);
        corrupt_session_pin(&database_path, *first_session);

        let report = refresh(&persistence, ToolRegistryChangeImpactV1::EveryCurrentScope);
        assert_eq!(report.failures.len(), 1, "exactly the injected fault is reported");
        let failure = &report.failures[0];
        assert_eq!(failure.scope, first);
        assert_eq!(failure.stage, "build", "the failure names where it stopped");
        assert!(!failure.reason.is_empty());
        assert!(report.refreshed.contains(&second), "the other Session must still be refreshed");
        assert!(report.refreshed.contains(&scopes[0]), "the Workspace must still be refreshed");
        assert_eq!(
            head(&persistence, &first),
            before_first,
            "the failing scope keeps its previous head rather than a half-written one"
        );
        assert_ne!(
            head(&persistence, &second).tool_registry_snapshot_id,
            before_second.tool_registry_snapshot_id
        );

        // Recovery: once the bad row is gone, the next legitimate change refreshes the
        // scope that failed, so it cannot stay stale.
        repair_session_pins(&database_path, *first_session);
        let recovered = refresh(&persistence, ToolRegistryChangeImpactV1::EveryCurrentScope);
        assert!(recovered.is_clean(), "{:?}", failure_reasons(&recovered));
        assert!(recovered.refreshed.contains(&first), "the recovered scope is refreshed again");
        assert_ne!(
            head(&persistence, &first).tool_registry_snapshot_id,
            before_first.tool_registry_snapshot_id
        );
    }
}
