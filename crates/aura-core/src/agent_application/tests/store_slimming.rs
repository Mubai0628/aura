//! What turns leave in the store is what is read again: one record of each event and no copy of
//! it, transport records that only say their state and time, at most two full request snapshots
//! per conversation (the newest and the newest the provider counted), and no raw stream of a reply
//! that was recorded. An older step's context details still show its figures.
use super::*;
use aura_agent_domain::AgentInspectorRequest;
use aura_core_broker::agent_store;
use aura_projection_contract::AgentTurnView;

async fn finished_turn(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
) -> AgentRunProjection {
    let run = waiting::start(runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    waiting::wait(runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await
}

fn agent_of(runtime: &crate::RuntimeState) -> Arc<AgentApplication> {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turns_leave_only_what_is_read_again() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    for _ in 0..3 {
        finished_turn(&runtime, session, model).await;
    }
    let agent = agent_of(&runtime);
    let turns: Vec<AgentTurnView> = agent
        .history(AgentHistoryRequest {
            session_id: session,
            before_turn_task_id: None,
            within_turn: None,
            limit: 10,
        })
        .await
        .expect("history")
        .turns;
    let first_step = turns[0].assistant_messages[0].model_step_id;
    let counted = agent
        .store
        .read(agent_store::StoreStep::operation("test.store_counts"), move |db| {
            let count = |sql: &str| -> Result<i64, aura_persistence_core::PersistenceCoreError> {
                Ok(db.query_row(sql, [], |row| row.get(0))?)
            };
            Ok((
                count("SELECT count(*) FROM projection_outbox")?,
                count("SELECT count(*) FROM events WHERE kind='model.transport'")?,
                count(
                    "SELECT coalesce(max(length(payload)), 0) FROM events \
                     WHERE kind='model.transport'",
                )?,
                db.query_row(
                    "SELECT count(*) FROM context_snapshots WHERE session_id=?1",
                    [session.to_string()],
                    |row| row.get::<_, i64>(0),
                )?,
                count("SELECT count(*) FROM artifacts WHERE media_type='text/event-stream'")?,
                count(
                    "SELECT count(*) FROM artifacts \
                     WHERE media_type LIKE 'application/vnd.aura.context-section%'",
                )?,
                count(
                    "SELECT coalesce(sum(json_array_length(payload, '$.sections')), 0) \
                     FROM context_snapshots",
                )?,
            ))
        })
        .await
        .expect("the store's counts");
    let (outbox, transports, largest_transport, snapshots, raw_streams, sections, kept_sections) =
        counted;
    assert_eq!(outbox, 0, "an event is written once, with no copy");
    assert!(transports > 0, "each attempt still says how its state changed");
    assert!(largest_transport < 1024, "a transport record of {largest_transport} bytes");
    assert!(snapshots <= 2, "{snapshots} full snapshots of one conversation");
    assert_eq!(raw_streams, 0, "a recorded reply keeps no raw stream");
    assert_eq!(sections, kept_sections, "section texts only of the snapshots kept");

    let inspection = agent
        .inspector(AgentInspectorRequest { session_id: session, model_step_id: first_step })
        .await
        .expect("the first step's context details");
    assert!(!inspection.projection.sections.is_empty(), "its figures are still shown");
    assert!(inspection.context.is_none(), "its full snapshot is no longer kept");
}

/// Each answer's step, oldest first.
async fn steps(runtime: &crate::RuntimeState, session: SessionId) -> Vec<ModelStepId> {
    let turns = agent_of(runtime)
        .history(AgentHistoryRequest {
            session_id: session,
            before_turn_task_id: None,
            within_turn: None,
            limit: 10,
        })
        .await
        .expect("history")
        .turns;
    let mut steps = Vec::new();
    for message in turns.iter().flat_map(|turn| &turn.assistant_messages) {
        if !steps.contains(&message.model_step_id) {
            steps.push(message.model_step_id);
        }
    }
    steps
}

#[derive(Debug, Default)]
struct Leftovers {
    transport_events: u64,
    raw_streams: u64,
    snapshots: Vec<String>,
}

/// What an older version kept beside the same records: an outbox copy of every event, the whole
/// record of each call in a `model.transport` event, every completed call's raw stream, and a
/// snapshot with its section texts for each earlier step, older than the ones kept.
fn add_older_versions_leftovers(
    root: &std::path::Path,
    session: SessionId,
    steps: &[ModelStepId],
) -> Leftovers {
    use aura_model_domain::{
        TransportAttemptRecord, TransportAttemptState, TransportOperationContext,
    };
    use aura_persistence_core::{artifact, event, record};
    let mut store =
        aura_persistence_core::DatabaseConnection::open_existing(&root.join("aura-v2.db"))
            .expect("the stopped Core's store");
    store
        .unit_of_work(|tx| -> aura_persistence_core::StoreResult<Leftovers> {
            let at = UtcTimestamp::now();
            let mut added = Leftovers::default();
            tx.execute(
                "INSERT INTO projection_outbox(event_id,topic,revision,payload) \
                 SELECT event_id,'old',sequence,payload FROM events",
                [],
            )?;
            let calls = tx
                .prepare("SELECT id, payload FROM transport_attempts")?
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            for (id, payload) in calls {
                let mut attempt: TransportAttemptRecord = record::decode(&payload)?;
                let TransportOperationContext::AgentModelAttempt { model_step_id, .. } =
                    attempt.request_evidence.operation_context.clone()
                else {
                    continue;
                };
                let owner = model_step_id.to_string();
                event::append(
                    tx,
                    &event::EventDraft {
                        id: EventId::new(),
                        owner: &owner,
                        kind: "model.transport",
                        version: 1,
                        payload: &serde_json::to_value(&attempt)?,
                        at,
                    },
                    "model",
                )?;
                added.transport_events += 1;
                if attempt.state == TransportAttemptState::Completed
                    && attempt.raw_stream_artifact_id.is_none()
                {
                    let stream = ArtifactId::new();
                    let bytes = format!("data: {id}");
                    artifact::put(tx, stream, &owner, "text/event-stream", bytes.as_bytes(), at)?;
                    attempt.raw_stream_artifact_id = Some(stream);
                    tx.execute(
                        "UPDATE transport_attempts SET payload=?2 WHERE id=?1",
                        rusqlite::params![id, record::encode(&attempt)?],
                    )?;
                    added.raw_streams += 1;
                }
            }
            let (newest, registry): (String, Option<String>) = tx.query_row(
                "SELECT payload, registry_snapshot_id FROM context_snapshots \
                 WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1",
                [session.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let template: Value = serde_json::from_str(&newest)?;
            let kept = tx
                .prepare(
                    "SELECT json_extract(payload,'$.model_step_id') FROM context_snapshots \
                     WHERE session_id=?1",
                )?
                .query_map([session.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut rowid: i64 =
                tx.query_row("SELECT min(rowid) FROM context_snapshots", [], |row| row.get(0))?;
            for step in steps.iter().rev().filter(|step| !kept.contains(&step.to_string())) {
                rowid -= 1;
                let id = ContextProjectionSnapshotId::new().to_string();
                let mut payload = template.clone();
                payload["id"] = json!(id);
                payload["model_step_id"] = json!(step);
                for section in payload["sections"].as_array_mut().into_iter().flatten() {
                    let text = ArtifactId::new();
                    artifact::put(tx, text, &step.to_string(), "text/plain", id.as_bytes(), at)?;
                    section["content_artifact_id"] = json!(text);
                }
                tx.execute(
                    "INSERT INTO context_snapshots(rowid,id,session_id,registry_snapshot_id,payload) \
                     VALUES(?1,?2,?3,?4,?5)",
                    rusqlite::params![
                        rowid,
                        id,
                        session.to_string(),
                        registry,
                        record::encode(&payload)?
                    ],
                )?;
                added.snapshots.push(id);
            }
            Ok(added)
        })
        .expect("an older version's leftovers")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_store_slimmed_before_core_starts_serves_the_conversation_on() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    for _ in 0..3 {
        finished_turn(&runtime, session, model).await;
    }
    let earlier = steps(&runtime, session).await;
    host.stop(runtime).await;
    let root = host.paths.data_dir.join("v2-execution");
    let added = add_older_versions_leftovers(&root, session, &earlier);
    assert!(!added.snapshots.is_empty(), "{added:?}");

    let report = aura_persistence_v2::slimming::slim(&root, &|_| Ok(u64::MAX / 4), &mut |_| {
        std::ops::ControlFlow::Continue(())
    })
    .expect("slimming the stopped Core's store");
    assert!(report.outbox_rows > 0, "{report:?}");
    assert_eq!(report.transport_events, added.transport_events, "{report:?}");
    assert_eq!(report.raw_streams, added.raw_streams, "{report:?}");
    assert!(report.snapshots >= added.snapshots.len() as u64, "{report:?}");
    assert_eq!(aura_persistence_v2::slimming::owed(&root), None);

    let runtime = host.start().await;
    finished_turn(&runtime, session, model).await;
    let agent = agent_of(&runtime);
    let all = steps(&runtime, session).await;
    assert_eq!(all.len(), earlier.len() + 1, "the next turn was measured and answered");
    let first = agent
        .inspector(AgentInspectorRequest { session_id: session, model_step_id: earlier[0] })
        .await
        .expect("the first step's context details");
    assert!(!first.projection.sections.is_empty(), "its figures are still shown");
    assert!(first.context.is_none());
    let newest_step = *all.last().expect("the newest step");
    let newest = agent
        .inspector(AgentInspectorRequest { session_id: session, model_step_id: newest_step })
        .await
        .expect("the newest step's context details");
    assert!(newest.context.is_some(), "the newest step keeps its full snapshot");
    let left = agent
        .store
        .read(agent_store::StoreStep::operation("test.store_counts"), move |db| {
            Ok(db
                .prepare("SELECT id FROM context_snapshots WHERE session_id=?1")?
                .query_map([session.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .await
        .expect("the snapshots left");
    assert!(left.len() <= 2, "{left:?}");
    assert!(
        left.iter().all(|id| !added.snapshots.contains(id)),
        "an older version's snapshot stayed"
    );
}

/// Deltas that arrive together are stored together: one commit and one stored delta for what
/// arrives within the batch window, where each delta took a commit and three records of its own.
/// Once the reply is sealed none of them is left.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deltas_that_arrive_together_are_stored_together_until_the_reply_is_sealed() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::DeltasThenHold).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let run = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let whole: String =
        (0..provider_fixture::HELD_DELTAS).map(|n| format!("piece {n}. ")).collect();
    let agent = agent_of(&runtime);
    let started = std::time::Instant::now();
    let step = loop {
        let turns = agent
            .history(AgentHistoryRequest {
                session_id: session,
                before_turn_task_id: None,
                within_turn: None,
                limit: 10,
            })
            .await
            .expect("history")
            .turns;
        let shown =
            turns.iter().flat_map(|turn| &turn.assistant_messages).find(|m| m.text == whole);
        if let Some(message) = shown {
            assert!(message.partial, "the step is still running");
            break message.model_step_id;
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(20), "{turns:?}");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    let stored = agent
        .store
        .read(agent_store::StoreStep::operation("test.stored_deltas"), move |db| {
            Ok(db.query_row(
                "SELECT count(*) FROM events WHERE owner=?1 AND kind='agent.visible_delta'",
                [step.to_string()],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .await
        .expect("the stored deltas");
    assert!(
        stored <= 3,
        "{stored} stored deltas for {} that arrived together",
        provider_fixture::HELD_DELTAS
    );
    provider.release_window.notify_one();
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    waiting::wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let left = agent
        .store
        .read(agent_store::StoreStep::operation("test.streamed_rows"), move |db| {
            Ok(db.query_row(
                "SELECT (SELECT count(*) FROM events WHERE owner=?1 AND kind IN \
                 ('agent.visible_delta','agent.visible_delta_index','agent.visible_delta_epoch')) \
                 + (SELECT count(*) FROM agent_visible_delta WHERE owner=?1) \
                 + (SELECT count(*) FROM agent_visible_index WHERE owner=?1) \
                 + (SELECT count(*) FROM agent_visible_coverage WHERE owner=?1)",
                [step.to_string()],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .await
        .expect("the streamed rows");
    assert_eq!(left, 0, "a sealed reply keeps none of the deltas it streamed");
}
