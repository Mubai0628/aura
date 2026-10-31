//! Deleting a conversation with a long history does not hold the execution store: while its
//! records go, another write waits for one deletion step at most, where it used to wait for the
//! whole deletion (up to 14 seconds in the owner's development store).
use super::*;
use aura_core_broker::agent_store::StoreStep;
use std::time::{Duration, Instant};

/// Events filed under the conversation, written straight to the store.
const FILLER: usize = 30_000;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_long_deletion_lets_other_writes_in_between_its_steps() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::new(true).await;
    let (runtime, session, _) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let owner = format!("agent-session:{session}");
    let payload = json!({ "text": "a long conversation ".repeat(10) });
    for _ in 0..FILLER / 2_500 {
        services
            .broker
            .lock()
            .expect("the store")
            .database
            .unit_of_work(|tx| {
                for _ in 0..2_500 {
                    let draft = aura_persistence_core::event::EventDraft {
                        id: EventId::new(),
                        owner: &owner,
                        kind: "test.long_history",
                        version: 1,
                        payload: &payload,
                        at: UtcTimestamp::now(),
                    };
                    aura_persistence_core::event::append(tx, &draft, "test.long_history")?;
                }
                Ok::<_, aura_persistence_core::PersistenceCoreError>(())
            })
            .expect("a long history");
    }
    let path = services.broker.database_path().expect("the store's path");
    let left = || -> i64 {
        aura_persistence_core::open_reader(&path)
            .expect("a reader")
            .query_row("SELECT count(*) FROM events WHERE owner=?1", [&owner], |row| row.get(0))
            .expect("the count")
    };
    assert_eq!(left(), FILLER as i64);

    crate::r03_tests::success(&runtime, "session.archive", json!({"session_id": session})).await;
    let asked = Instant::now();
    crate::r03_tests::success(&runtime, "session.delete", json!({"session_id": session})).await;
    let answered = asked.elapsed();

    // While the records go, a write of something else gets the store between steps.
    let (mut longest, mut probes) = (Duration::ZERO, 0);
    let deadline = Instant::now() + Duration::from_secs(120);
    while left() > 0 && Instant::now() < deadline {
        let started = Instant::now();
        services
            .agent_store
            .write(StoreStep::operation("test.probe"), |journal| {
                journal.database.unit_of_work(|tx| {
                    Ok::<i64, aura_persistence_core::PersistenceCoreError>(tx.query_row(
                        "SELECT count(*) FROM sessions",
                        [],
                        |row| row.get(0),
                    )?)
                })
            })
            .await
            .expect("the probe write");
        longest = longest.max(started.elapsed());
        probes += 1;
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    crate::session_lifecycle::deleted(&runtime).await;
    eprintln!(
        "delete answered in {answered:?}; {probes} writes meanwhile, the longest {longest:?}"
    );
    assert_eq!(left(), 0, "the conversation's events are gone");
    assert!(answered < Duration::from_secs(2), "the route answers once the conversation is hidden");
    assert!(probes >= 3, "the deletion ran long enough to measure ({probes} writes)");
    assert!(longest < Duration::from_millis(500), "a write waited {longest:?} for the deletion");
}
