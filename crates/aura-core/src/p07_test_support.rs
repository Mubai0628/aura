use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use aura_context_fixtures::{P06PreparedFixture, P06ToolContextFixtureInputV1};
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    AgentRunId, CorrelationId, EndpointId, ModelProfileId, ModelStepId, ProviderId, Revision,
    RuntimeInstanceId, SessionId, TurnTaskId, UtcTimestamp, WorkspaceId,
};
use aura_persistence::{
    ContextArtifactDraft, ContextProjectionCommitRequest, ContextSourceEventCommitRequest,
    EventActor, PersistenceCoordinator, PersistenceOptions, PersistenceStartup,
    PersistentEventDraft, PromptBundleCommitRequest, ToolRegistryBuildRequestV1,
};
use aura_security_contract::PermissionMode;
use aura_tool_contract::{ToolRegistryScopeV1, ToolSettingsSnapshotV1};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};
use rusqlite::{Connection, params};
use tempfile::TempDir;

use aura_tool_service::p07_registry::ToolRegistryControlServiceV1;

fn commit_initial_p06_context_fixture(
    persistence: &PersistenceCoordinator,
    fixture: &P06PreparedFixture,
) -> Result<()> {
    persistence.commit_prompt_bundle_snapshot(PromptBundleCommitRequest {
        session_id: fixture.session_id,
        snapshot: fixture.prompt_bundle_snapshot.clone(),
        artifacts: fixture
            .prompt_bundle_artifacts
            .iter()
            .map(|artifact| ContextArtifactDraft {
                artifact_id: artifact.artifact_id,
                media_type: artifact.media_type.clone(),
                bytes: artifact.bytes.clone(),
            })
            .collect(),
        correlation_id: CorrelationId::new(),
    })?;
    persistence.commit_context_source_event(ContextSourceEventCommitRequest {
        event: PersistentEventDraft {
            event_id: fixture.current_user_event_id,
            workspace_id: Some(fixture.workspace_id),
            session_id: Some(fixture.session_id),
            turn_task_id: Some(fixture.turn_task_id),
            agent_run_id: Some(fixture.agent_run_id),
            event_type: "RootUserMessageAccepted".to_owned(),
            event_version: 1,
            actor: EventActor::User,
            correlation_id: CorrelationId::new(),
            causation_id: None,
            atomic_group_id: None,
            payload_json: None,
            payload_artifact_ref: None,
            searchable_text: Some(fixture.current_user_content.clone()),
            created_at: UtcTimestamp::now(),
        },
        canonical_content: fixture.current_user_content.clone(),
        model_visible: true,
        explicitly_user_authored: true,
    })?;
    commit_p06_projection_fixture(persistence, fixture)
}

pub(crate) fn commit_p06_projection_fixture(
    persistence: &PersistenceCoordinator,
    fixture: &P06PreparedFixture,
) -> Result<()> {
    persistence.commit_context_projection_snapshot(ContextProjectionCommitRequest {
        snapshot: fixture.prepared.context.snapshot.clone(),
        execution_snapshot: fixture.execution_snapshot.clone(),
        instruction_envelope: fixture.prepared.context.instruction_envelope.clone(),
        model_egress_manifest: fixture.prepared.context.model_egress_manifest.clone(),
        artifacts: fixture
            .prepared
            .artifacts
            .iter()
            .map(|artifact| ContextArtifactDraft {
                artifact_id: artifact.artifact_id,
                media_type: artifact.media_type.clone(),
                bytes: artifact.bytes.clone(),
            })
            .collect(),
        prompt_inspector: fixture.prepared.prompt_inspector.clone(),
        search_chunks: Vec::new(),
        correlation_id: CorrelationId::new(),
    })?;
    Ok(())
}

pub(crate) fn seed_session_scope(
    persistence: &PersistenceCoordinator,
    database_path: &Path,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    turn_task_id: TurnTaskId,
    agent_run_id: AgentRunId,
    root: &Path,
) -> Result<()> {
    let canonical_root = fs::canonicalize(root).context("canonicalize P07 data root")?;
    let root_wire = canonical_root.display().to_string();
    persistence.upsert_workspace_session(
        WorkspaceSummary {
            workspace_id,
            display_name: "P07 authorized Aura Stdio fixture".to_owned(),
            selected_root: root_wire.clone(),
            canonical_root: root_wire,
            root_identity: None,
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        },
        SessionSummary {
            session_id,
            workspace_id,
            title: "P07 authorized Aura Stdio fixture".to_owned(),
            state: "running".to_owned(),
        },
        CorrelationId::new(),
    )?;
    persistence.ensure_session_permission_state(
        session_id,
        workspace_id,
        PermissionMode::FullAccess,
        CorrelationId::new(),
    )?;
    let connection = Connection::open(database_path).context("open P07 fixture database")?;
    connection.execute_batch("PRAGMA foreign_keys=ON;")?;
    let now = UtcTimestamp::now().to_string();
    connection.execute(
        "INSERT INTO turn_tasks(turn_task_id, session_id, state, created_at, updated_at)
         VALUES (?1, ?2, 'running', ?3, ?3)",
        params![turn_task_id.to_string(), session_id.to_string(), now],
    )?;
    connection.execute(
        "INSERT INTO agent_runs(
            agent_run_id, turn_task_id, parent_agent_run_id, state, created_at, updated_at
         ) VALUES (?1, ?2, NULL, 'running', ?3, ?3)",
        params![agent_run_id.to_string(), turn_task_id.to_string(), now],
    )?;
    Ok(())
}

pub(crate) fn registry_request(scope: ToolRegistryScopeV1) -> ToolRegistryBuildRequestV1 {
    ToolRegistryBuildRequestV1 {
        scope,
        settings: ToolSettingsSnapshotV1::default(),
        mode_permission_context_hash: CanonicalHashV1::sha256(
            b"P07 authorized Aura Stdio permission context",
        ),
        core_exposed_tool_ids: Default::default(),
        non_package_candidates: Vec::new(),
        created_at: UtcTimestamp::now(),
        correlation_id: CorrelationId::new(),
    }
}

/// A V1 tool authority holding only the signed Built-in release, one Session scope and
/// one committed P06 context. The credential lifecycle tests target the Built-in
/// `http.request` instance (slot `api_key`, bearer placement); the P07 package fixture they
/// used before is retired.
pub(crate) struct BuiltinHttpCredentialFixtureV1 {
    _root: TempDir,
    pub(crate) persistence: PersistenceCoordinator,
    pub(crate) instance: aura_tool_domain::ToolInstanceRecordV1,
    pub(crate) p06_initial: P06PreparedFixture,
}

impl BuiltinHttpCredentialFixtureV1 {
    pub(crate) fn new() -> Result<Self> {
        let root = tempfile::tempdir().context("create the credential fixture root")?;
        let database_path = root.path().join("aura.db");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            database_path.clone(),
            root.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .context("start the credential fixture persistence")?
        {
            PersistenceStartup::Ready(value) => value,
            PersistenceStartup::RecoveryRequired(value) => {
                bail!("unexpected credential fixture recovery requirement: {}", value.reason)
            }
        };
        persistence
            .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())?;
        let owner = aura_tool_registry::agent_release::release()?
            .entry("http.request")
            .context("the Built-in HTTP credential owner")?
            .instance
            .tool_instance_id;
        let instance = persistence
            .tool_authority_snapshot(None, None, None)?
            .instances
            .into_iter()
            .find(|instance| instance.tool_instance_id == owner)
            .context("the materialized Built-in http.request instance")?;
        let workspace_id = WorkspaceId::new();
        let session_id = SessionId::new();
        let turn_task_id = TurnTaskId::new();
        let agent_run_id = AgentRunId::new();
        seed_session_scope(
            &persistence,
            &database_path,
            workspace_id,
            session_id,
            turn_task_id,
            agent_run_id,
            root.path(),
        )?;
        let scope =
            ToolRegistryScopeV1::Session { workspace_id, session_id, turn_task_id, agent_run_id };
        let initial =
            ToolRegistryControlServiceV1.build_scope(&persistence, registry_request(scope))?;
        let corpus = persistence
            .read_tool_registry_search_corpus(&initial.commit.registry_snapshot_ref)?
            .context("the initial Registry Snapshot")?;
        let p06_initial = aura_context_fixtures::p06_prepared_tool_context_fixture(
            P06ToolContextFixtureInputV1 {
                workspace_id,
                session_id,
                turn_task_id,
                agent_run_id,
                model_step_id: ModelStepId::new(),
                model_profile_id: ModelProfileId::new(),
                provider_id: ProviderId::new(),
                endpoint_id: EndpointId::new(),
                tool_schema_set: corpus.snapshot.schema_set.clone(),
                effective_tool_bindings: corpus.snapshot.effective_bindings.clone(),
            },
        )?;
        commit_initial_p06_context_fixture(&persistence, &p06_initial)?;
        Ok(Self { _root: root, persistence, instance, p06_initial })
    }
}
