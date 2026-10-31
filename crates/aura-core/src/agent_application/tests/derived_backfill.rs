//! The background copy of derived rows on a store with nothing to copy, and on a stream that
//! appears while Core runs. Finding the next stream used to read every event of every step that
//! never streamed text, inside a write section on the one store owner, on every pass.
use super::*;
use aura_core_base::sync_boundary;
use aura_core_broker::agent_store;
use std::time::Duration;

fn agent(runtime: &crate::RuntimeState) -> Arc<crate::agent_application::AgentApplication> {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required()
}

fn backfill_spans(runtime: &crate::RuntimeState) -> Vec<sync_boundary::SyncSpan> {
    runtime
        .sync
        .spans()
        .into_iter()
        .filter(|span| span.method.as_deref().is_some_and(|m| m.starts_with("agent.backfill.")))
        .collect()
}

async fn until(deadline: Duration, predicate: impl Fn() -> bool) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < deadline {
        if predicate() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    predicate()
}

/// A new store has nothing to copy, so a pass only reads. Each pass used to take the write
/// section twice, and the stream half of it read the whole journal of steps without a stream.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_idle_backfill_takes_no_write_section() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    // The first pass runs 200 ms after the start and the next one after the idle pause.
    let passes = || {
        backfill_spans(&runtime)
            .iter()
            .filter(|span| {
                span.method.as_deref().is_some_and(|m| m.starts_with("agent.backfill.visible"))
            })
            .count()
    };
    assert!(until(Duration::from_secs(15), || passes() >= 2).await, "two passes ran");
    let writes: Vec<_> = backfill_spans(&runtime)
        .into_iter()
        .filter(|span| span.section == sync_boundary::SyncSection::AgentStore.as_str())
        .map(|span| (span.method, span.elapsed_ms))
        .collect();
    assert!(writes.is_empty(), "an idle pass took the store's write section: {writes:?}");
}

/// A stream that appears after the first sweep is found from the events added since, and the
/// write section is taken to copy it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stream_that_appears_while_core_runs_is_still_copied() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Hold).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let run = start.run.agent_run_id;
    let agent = agent(&runtime);
    assert!(
        until(Duration::from_secs(20), || {
            !provider.requests.lock().expect("provider requests").is_empty()
        })
        .await,
        "the step is waiting for its answer"
    );
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { unreachable!() };
    let owner = {
        let journal = services.broker.lock().expect("journal");
        let connection = journal.database.connection();
        let (step, attempt): (String, String) = connection
            .query_row(
                "SELECT s.id, a.id FROM model_steps s \
                 JOIN transport_attempts a ON a.snapshot_id=s.snapshot_id \
                 WHERE s.agent_run_id=?1 ORDER BY a.rowid DESC LIMIT 1",
                [run.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the running step and its attempt");
        // Deltas as a version before the derived tables wrote them: events only.
        for (sequence, text) in [(1, "old "), (2, "stream "), (3, "text")] {
            connection
                .execute(
                    "INSERT INTO events(event_id, owner, kind, version, payload, created_at) \
                     VALUES(?1, ?2, 'agent.visible_delta', 1, \
                     json_object('attempt', ?3, 'sequence', ?4, 'text', ?5, 'reasoning', ''), \
                     '2026-10-27T00:00:00Z')",
                    rusqlite::params![EventId::new().to_string(), step, attempt, sequence, text],
                )
                .expect("a legacy delta");
        }
        step
    };
    let covered = || {
        let journal = services.broker.lock().expect("journal");
        journal
            .database
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_visible_coverage WHERE owner=?1)",
                [&owner],
                |row| row.get::<_, bool>(0),
            )
            .expect("coverage")
    };
    assert!(until(Duration::from_secs(20), covered).await, "the stream was copied and indexed");
    let copied = agent.store().receipts().into_iter().any(|receipt| {
        receipt.operation == "agent.backfill.visible_stream"
            && receipt.state == agent_store::ReceiptState::Settled { committed: true, code: None }
    });
    assert!(copied, "the copy ran in the backfill's own write section");
    let journal = services.broker.lock().expect("journal");
    let text: String = journal
        .database
        .connection()
        .query_row(
            "SELECT group_concat(text, '') FROM \
             (SELECT text FROM agent_visible_delta WHERE owner=?1 ORDER BY sequence)",
            [&owner],
            |row| row.get(0),
        )
        .expect("copied text");
    assert_eq!(text, "old stream text");
}
