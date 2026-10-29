//! Reads have read-only connections of their own and a lane of their own: a slow read holds up
//! no other read, and writes queued for the one write connection hold up no read.
use super::super::*;
use std::time::{Duration, Instant};

/// How long the slow step holds its connection.
const HELD: Duration = Duration::from_secs(4);
/// A read that is not held up answers well inside this, even on a loaded machine.
const QUICK: Duration = Duration::from_millis(1500);

fn store_of(runtime: &crate::RuntimeState) -> Arc<AgentStore> {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core is ready")
    };
    Arc::clone(services.agent_required().store())
}

fn select_one(db: &Connection) -> Result<i64, PersistenceCoreError> {
    Ok(db.query_row("SELECT 1", [], |row| row.get(0))?)
}

async fn started(inside: tokio::sync::oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(10), inside)
        .await
        .expect("the slow step starts")
        .expect("the slow step says so");
}

/// How long a quick read waited.
async fn quick_read(store: &Arc<AgentStore>) -> Duration {
    let asked = Instant::now();
    let value =
        store.read(StoreStep::operation("test.quick_read"), select_one).await.expect("the read");
    assert_eq!(value, 1);
    asked.elapsed()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_read_holds_up_no_other_read() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let store = store_of(&runtime);
    let (entered, inside) = tokio::sync::oneshot::channel();
    let slow = tokio::spawn({
        let store = Arc::clone(&store);
        async move {
            store
                .read(StoreStep::operation("test.slow_read"), move |db| {
                    let _ = entered.send(());
                    std::thread::sleep(HELD);
                    select_one(db)
                })
                .await
        }
    });
    started(inside).await;
    let waited = quick_read(&store).await;
    assert!(waited < QUICK, "the quick read waited {waited:?} behind the slow one");
    slow.await.expect("joined").expect("the slow read");
    let kept = store.readers.as_ref().map_or(0, |readers| readers.kept());
    assert!(kept >= 2, "the quick read had a connection of its own: {kept} kept");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_queued_for_the_one_writer_hold_up_no_read() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let store = store_of(&runtime);
    let (entered, inside) = tokio::sync::oneshot::channel();
    let mut writes = vec![tokio::spawn({
        let store = Arc::clone(&store);
        async move {
            store
                .write(StoreStep::operation("test.slow_write"), move |_store| {
                    let _ = entered.send(());
                    std::thread::sleep(HELD);
                    Ok(())
                })
                .await
        }
    })];
    started(inside).await;
    // The slow write holds the writer and the rest queue for it: every shared worker is taken.
    for _ in 0..crate::sync_boundary::SYNC_WORKERS {
        let store = Arc::clone(&store);
        writes.push(tokio::spawn(async move {
            store.write(StoreStep::operation("test.queued_write"), |_store| Ok(())).await
        }));
    }
    let full = tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.sync.available() > 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(full.is_ok(), "every shared worker is taken");
    let waited = quick_read(&store).await;
    assert!(waited < QUICK, "the read waited {waited:?} behind the queued writes");
    for write in writes {
        write.await.expect("joined").expect("the write");
    }
}
