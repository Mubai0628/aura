use std::{hint::black_box, time::Instant};

use aura_context_domain::{ContextPrivacyClass, ContextSearchFilters};
use aura_context_fixtures::{FixtureArtifact, P06PreparedFixture, p06_prepared_context_fixture};
use aura_domain_common::{
    ContentHash, CorrelationId, Revision, RuntimeInstanceId, SessionSequence, UtcTimestamp,
};
use aura_persistence::{
    ContextArtifactDraft, ContextProjectionCommitRequest, ContextSearchChunkDraft,
    ContextSearchStoredQuery, ContextSourceEventCommitRequest, EventActor, PersistenceCoordinator,
    PersistenceOptions, PersistenceStartup, PersistentEventDraft, PromptBundleCommitRequest,
};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};
use rusqlite::{Connection, params};
use serde_json::json;

const CHUNK_COUNT: u64 = 100_000;
const QUERY_ITERATIONS: usize = 31;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let database_path = temporary.path().join("p06-fts-scale.sqlite3");
    let artifact_root = temporary.path().join("artifacts");
    let fixture = p06_prepared_context_fixture()?;
    let persistence = start(database_path.clone(), artifact_root)?;
    seed_scope(&persistence, &database_path, &fixture)?;
    commit_prompt_bundle(&persistence, &fixture)?;
    commit_current_user_event(&persistence, &fixture)?;

    let chunks = (1..=CHUNK_COUNT)
        .map(|sequence| {
            let searchable_text = if sequence % 10_000 == 0 {
                format!("P06 exact phrase needle scale chunk {sequence}")
            } else {
                format!("P06 scale chunk {sequence}")
            };
            ContextSearchChunkDraft {
                agent_run_id: Some(fixture.agent_run_id),
                turn_task_id: Some(fixture.turn_task_id),
                source_type: "performance_fixture".to_owned(),
                source_id: format!("p06-scale-{sequence}"),
                session_sequence: SessionSequence::new(sequence),
                privacy_class: ContextPrivacyClass::ModelVisible,
                source_hash: ContentHash::sha256(searchable_text.as_bytes()),
                searchable_text,
            }
        })
        .collect::<Vec<_>>();
    let started = Instant::now();
    persistence.commit_context_projection_snapshot(ContextProjectionCommitRequest {
        snapshot: fixture.prepared.context.snapshot.clone(),
        execution_snapshot: fixture.execution_snapshot.clone(),
        instruction_envelope: fixture.prepared.context.instruction_envelope.clone(),
        model_egress_manifest: fixture.prepared.context.model_egress_manifest.clone(),
        artifacts: fixture.prepared.artifacts.clone().into_iter().map(context_artifact).collect(),
        prompt_inspector: fixture.prepared.prompt_inspector.clone(),
        search_chunks: chunks,
        correlation_id: CorrelationId::new(),
    })?;
    let index_commit_ms = started.elapsed().as_secs_f64() * 1_000.0;

    let mut query_samples = Vec::with_capacity(QUERY_ITERATIONS);
    let mut last_page = None;
    for _ in 0..QUERY_ITERATIONS {
        let started = Instant::now();
        let page = persistence.search_context_rows(ContextSearchStoredQuery {
            session_id: fixture.session_id,
            agent_run_id: fixture.agent_run_id,
            fts_query: "\"exact phrase needle\"".to_owned(),
            filters: ContextSearchFilters::default(),
            cutoff: None,
            after: None,
            fetch_limit: 32,
        })?;
        query_samples.push(started.elapsed().as_secs_f64() * 1_000.0);
        black_box(&page);
        last_page = Some(page);
    }
    query_samples.sort_by(f64::total_cmp);
    let last_page = last_page.ok_or("FTS performance query did not execute")?;
    let passed = last_page.rows.len() == 10
        && last_page.index_complete_through == SessionSequence::new(CHUNK_COUNT)
        && last_page.cutoff == SessionSequence::new(CHUNK_COUNT);
    if !passed {
        return Err("100K FTS scale integrity gate failed".into());
    }
    let p95_index = ((query_samples.len() * 95).div_ceil(100)).saturating_sub(1);
    let database_bytes = std::fs::metadata(&database_path)?.len();
    let report = json!({
        "schema_version": 1,
        "profile": "release",
        "provider_requests": 0,
        "measurement": {
            "name": "fts_scale_100k_chunks",
            "acceptance_type": "scale_integrity",
            "indexed_chunks": CHUNK_COUNT,
            "expected_exact_phrase_matches": 10,
            "actual_exact_phrase_matches": last_page.rows.len(),
            "index_complete_through": last_page.index_complete_through.value(),
            "index_commit_ms": round_millis(index_commit_ms),
            "query_iterations": QUERY_ITERATIONS,
            "query_median_ms": round_millis(query_samples[query_samples.len() / 2]),
            "query_p95_ms": round_millis(query_samples[p95_index]),
            "database_bytes": database_bytes,
            "latency_threshold": null,
            "passed": passed
        }
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn start(
    database_path: std::path::PathBuf,
    artifact_root: std::path::PathBuf,
) -> Result<PersistenceCoordinator, Box<dyn std::error::Error>> {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        database_path,
        artifact_root,
        RuntimeInstanceId::new(),
    ))? {
        PersistenceStartup::Ready(coordinator) => Ok(coordinator),
        PersistenceStartup::RecoveryRequired(recovery) => {
            Err(format!("unexpected recovery requirement: {}", recovery.reason).into())
        }
    }
}

fn seed_scope(
    persistence: &PersistenceCoordinator,
    database_path: &std::path::Path,
    fixture: &P06PreparedFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    persistence.upsert_workspace_session(
        WorkspaceSummary {
            workspace_id: fixture.workspace_id,
            display_name: "P06 FTS Performance".to_owned(),
            selected_root: r"C:\Aura Tests\P06 FTS Performance".to_owned(),
            canonical_root: r"C:\Aura Tests\P06 FTS Performance".to_owned(),
            root_identity: None,
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        },
        SessionSummary {
            session_id: fixture.session_id,
            workspace_id: fixture.workspace_id,
            title: "P06 FTS Performance".to_owned(),
            state: "running".to_owned(),
        },
        CorrelationId::new(),
    )?;
    let connection = Connection::open(database_path)?;
    connection.execute_batch("PRAGMA foreign_keys=ON;")?;
    let now = UtcTimestamp::now().to_string();
    connection.execute(
        "INSERT INTO turn_tasks(turn_task_id, session_id, state, created_at, updated_at)
         VALUES (?1, ?2, 'running', ?3, ?3)",
        params![fixture.turn_task_id.to_string(), fixture.session_id.to_string(), now],
    )?;
    connection.execute(
        "INSERT INTO agent_runs(
            agent_run_id, turn_task_id, parent_agent_run_id, state, created_at, updated_at
         ) VALUES (?1, ?2, NULL, 'running', ?3, ?3)",
        params![fixture.agent_run_id.to_string(), fixture.turn_task_id.to_string(), now],
    )?;
    Ok(())
}

fn commit_prompt_bundle(
    persistence: &PersistenceCoordinator,
    fixture: &P06PreparedFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    persistence.commit_prompt_bundle_snapshot(PromptBundleCommitRequest {
        session_id: fixture.session_id,
        snapshot: fixture.prompt_bundle_snapshot.clone(),
        artifacts: fixture
            .prompt_bundle_artifacts
            .clone()
            .into_iter()
            .map(fixture_artifact)
            .collect(),
        correlation_id: CorrelationId::new(),
    })?;
    Ok(())
}

fn commit_current_user_event(
    persistence: &PersistenceCoordinator,
    fixture: &P06PreparedFixture,
) -> Result<(), Box<dyn std::error::Error>> {
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
    Ok(())
}

fn fixture_artifact(artifact: FixtureArtifact) -> ContextArtifactDraft {
    ContextArtifactDraft {
        artifact_id: artifact.artifact_id,
        media_type: artifact.media_type,
        bytes: artifact.bytes,
    }
}

fn context_artifact(
    artifact: aura_context_runtime::PreparedContextArtifactDraft,
) -> ContextArtifactDraft {
    ContextArtifactDraft {
        artifact_id: artifact.artifact_id,
        media_type: artifact.media_type,
        bytes: artifact.bytes,
    }
}

fn round_millis(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}
