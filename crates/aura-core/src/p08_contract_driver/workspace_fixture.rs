//! Restart recovery for the compile-time isolated Tool settings fixture.
//! This never creates a product Session/TurnTask or changes production startup.

use anyhow::{Result, bail, ensure};
use aura_domain_common::{CorrelationId, WorkspaceId};
use aura_persistence::PersistenceCoordinator;
use aura_projection_contract::{ProjectionKind, ProjectionScope};
use aura_tool_contract::{ToolRegistryScopeV1, ToolSettingsSnapshotV1};

use crate::p07_registry::{ToolRegistryChangeImpactV1, ToolRegistryControlServiceV1};

pub(super) fn restore_registry(
    persistence: &PersistenceCoordinator,
    workspace_id: WorkspaceId,
) -> Result<()> {
    let heads = persistence.list_tool_registry_scope_heads()?;
    let mut has_workspace = false;
    let mut session_count = 0;
    let mut projection_missing = false;
    for (scope, _) in &heads {
        let (kind, projection_scope) = match scope {
            ToolRegistryScopeV1::Workspace { workspace_id: id } if *id == workspace_id => {
                has_workspace = true;
                (
                    ProjectionKind::WorkspaceToolRegistrySummary,
                    ProjectionScope::Workspace { workspace_id },
                )
            }
            ToolRegistryScopeV1::Session { workspace_id: id, session_id, .. }
                if *id == workspace_id =>
            {
                session_count += 1;
                ensure!(
                    persistence.tool_registry_scope_is_current(scope)?,
                    "isolated E2E fixture execution context is no longer current"
                );
                (
                    ProjectionKind::SessionEffectiveToolRegistry,
                    ProjectionScope::Session { session_id: *session_id },
                )
            }
            _ => continue,
        };
        projection_missing |= persistence.latest_tool_projection(kind, projection_scope)?.is_none();
    }
    // A partial or ambiguous fixture is an initialization failure, not permission
    // to invent a replacement execution context or leave the UI waiting forever.
    ensure!(
        has_workspace && session_count == 1,
        "isolated E2E fixture Registry heads are incomplete"
    );
    if !projection_missing {
        return Ok(());
    }
    let report = ToolRegistryControlServiceV1.refresh_current_scopes(
        persistence,
        ToolSettingsSnapshotV1::default(),
        ToolRegistryChangeImpactV1::Workspace(workspace_id),
        CorrelationId::new(),
    )?;
    if let Some(failure) = report.failures.first() {
        bail!("restore isolated E2E Registry at {}: {}", failure.stage, failure.reason);
    }
    ensure!(report.refreshed.len() == 2, "isolated E2E fixture Registry scopes were not restored");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::RuntimeInstanceId;
    use aura_persistence::{PersistenceOptions, PersistenceStartup};
    use std::path::Path;

    fn open(root: &Path, runtime: RuntimeInstanceId) -> PersistenceCoordinator {
        let startup = PersistenceCoordinator::start(PersistenceOptions::new(
            root.join("aura.sqlite3"),
            root.join("artifacts"),
            runtime,
        ))
        .expect("open fixture persistence");
        let PersistenceStartup::Ready(persistence) = startup else {
            panic!("fixture persistence requires recovery");
        };
        persistence
    }

    #[test]
    fn a_restarted_fixture_restores_both_registry_projections_without_reseeding_identities() {
        let root = tempfile::tempdir().expect("fixture directory");
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).expect("create workspace");
        let options = super::super::P08ContractDriverOptionsV1 {
            token: String::new(),
            workspace_fixture_root: Some(workspace),
        };
        let first = open(root.path(), RuntimeInstanceId::new());
        super::super::seed_isolated_workspace_session_v1(Some(&options), &first).expect("seed");
        let identities = first.workspace_session_projection().expect("identities");
        let original_heads = first.list_tool_registry_scope_heads().expect("heads");
        assert_eq!(original_heads.len(), 2);
        drop(first);

        let runtime = RuntimeInstanceId::new();
        let restarted = open(root.path(), runtime);
        let workspace_id = identities.workspaces[0].workspace_id;
        let scope = ProjectionScope::Workspace { workspace_id };
        assert!(
            restarted
                .latest_tool_projection(ProjectionKind::WorkspaceToolRegistrySummary, scope)
                .expect("read before restoration")
                .is_none()
        );
        super::super::seed_isolated_workspace_session_v1(Some(&options), &restarted)
            .expect("restore restarted fixture");
        assert_eq!(restarted.workspace_session_projection().expect("identities"), identities);
        let session_scope =
            ProjectionScope::Session { session_id: identities.sessions[0].session_id };
        for (kind, scope) in [
            (ProjectionKind::WorkspaceToolRegistrySummary, scope),
            (ProjectionKind::SessionEffectiveToolRegistry, session_scope),
        ] {
            let projection =
                restarted.latest_tool_projection(kind, scope).expect("read").expect("restored");
            assert_eq!(projection.runtime_instance_id(), runtime);
        }
        let restored_heads = restarted.list_tool_registry_scope_heads().expect("restored heads");
        for (scope, original) in &original_heads {
            assert!(restored_heads.iter().any(|(restored, _)| restored == scope));
            assert_eq!(
                restarted
                    .read_tool_registry_search_corpus(original)
                    .expect("old corpus")
                    .expect("old snapshot")
                    .snapshot
                    .snapshot_ref,
                *original
            );
        }
        // A duplicate call in the same Core neither adds identities nor advances heads.
        super::super::seed_isolated_workspace_session_v1(Some(&options), &restarted)
            .expect("idempotent");
        assert_eq!(restarted.list_tool_registry_scope_heads().expect("heads"), restored_heads);
    }

    #[test]
    fn a_partial_fixture_is_rejected_instead_of_claiming_its_registry_is_ready() {
        let root = tempfile::tempdir().expect("fixture directory");
        let persistence = open(root.path(), RuntimeInstanceId::new());
        assert!(restore_registry(&persistence, WorkspaceId::new()).is_err());
        assert!(persistence.list_tool_registry_scope_heads().expect("heads").is_empty());
    }
}
