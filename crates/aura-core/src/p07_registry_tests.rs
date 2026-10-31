//! p07_registry's tests, which build their fixtures with Core's `p07_test_support`. They read
//! the registry's names through `use super::*`, as they did inside its module; these imports
//! give them the same names.
use aura_domain_common::ErrorCategory;
use aura_persistence::PersistenceCoordinator;
use aura_projection_contract::ProjectionChange;
use aura_tool_contract::ToolRegistryErrorCode;
use aura_tool_contract::ToolRegistryScopeV1;
use aura_tool_contract::ToolRegistrySnapshotRef;
use aura_tool_contract::ToolSettingsSnapshotV1;
use aura_tool_service::p07_registry::*;

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
