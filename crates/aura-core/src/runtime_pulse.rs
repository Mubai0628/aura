//! The pulse Core sends Main every two seconds: its heartbeat (moved here from `lib.rs`) and,
//! the health projection again whenever the long work holding
//! Core's stores starts, changes or ends — so the status can say Core is busy, and why, while the
//! store is still held. Nothing here waits for the store.
use super::*;
use aura_core_base::{busy, outbound_watch};

pub(crate) fn spawn_heartbeat(
    state: Arc<RuntimeState>,
    queue: AsyncPriorityQueue,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = time::interval(HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        interval.tick().await;
        let mut shown = (None, false);
        loop {
            let due = interval.tick().await;
            let late = due.elapsed();
            let heartbeat = RuntimeHeartbeat {
                runtime_instance_id: state.options.runtime_instance_id,
                sequence: state.heartbeat_sequence.fetch_add(1, Ordering::AcqRel),
                sent_at: UtcTimestamp::now(),
            };
            let queueing = Instant::now();
            if send_notification(&queue, QueuePriority::P0, method::RUNTIME_HEARTBEAT, &heartbeat)
                .await
                .is_err()
            {
                break;
            }
            note_late_heartbeat(late, queueing.elapsed(), outbound_watch::current());
            // Until startup completes, its own projection says where Core is.
            if !state.startup_complete.load(Ordering::Acquire) {
                continue;
            }
            // And when the background store check finds damage.
            let now = (busy::current(), core_health::store_damaged(&state));
            if now == shown {
                continue;
            }
            shown = now;
            state.core_health_revision.fetch_add(1, Ordering::AcqRel);
            let projection = current_core_health_projection(&state);
            if send_notification(&queue, QueuePriority::P2, method::PROJECTION_CHANGE, &projection)
                .await
                .is_err()
            {
                break;
            }
        }
    })
}

/// A heartbeat this late, this long to queue, or behind a write this long, is worth a log line.
const LATE_HEARTBEAT: Duration = Duration::from_secs(1);

/// Logs a heartbeat that left late, waited to be queued, or sits behind a frame Main has not
/// read: Main's next "no heartbeat" can then be told apart as a stopped runtime or a full pipe.
/// The line is written off the pulse's own task, which a slow disk must not hold.
fn note_late_heartbeat(late: Duration, queued: Duration, writing: Option<(u64, u64)>) {
    let blocked = writing.is_some_and(|(ms, _)| ms >= LATE_HEARTBEAT.as_millis() as u64);
    if late < LATE_HEARTBEAT && queued < LATE_HEARTBEAT && !blocked {
        return;
    }
    let fields = serde_json::json!({
        "late_ms": late.as_millis() as u64,
        "queue_ms": queued.as_millis() as u64,
        "writing_ms": writing.map(|(ms, _)| ms),
        "writing_bytes": writing.map(|(_, bytes)| bytes),
    });
    drop(tokio::task::spawn_blocking(move || crate::append_core_log("heartbeat_late", fields)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_runtime_contract::CoreBusyActivityV1;

    /// The next core health projection the pulse sends, within `limit`.
    async fn next_health(queue: &AsyncPriorityQueue, limit: Duration) -> Value {
        time::timeout(limit, async {
            loop {
                let frame = queue.recv().await.expect("the pulse keeps its queue");
                let WireFrame::Control(bytes) = frame.frame else { continue };
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
                if value["method"] == method::PROJECTION_CHANGE
                    && value["params"]["projection_kind"] == "core_health"
                {
                    return value["params"].clone();
                }
            }
        })
        .await
        .expect("a health projection within the limit")
    }

    /// While long work holds the store, the health projection the pulse sends says Core is
    /// busy and with what; once the work ends, a new one says it no longer is.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_pulse_says_core_is_busy_while_long_work_holds_the_store() {
        let host = crate::r03_tests::Fixture::new();
        let state = Arc::new(host.start().await);
        state.startup_complete.store(true, Ordering::Release);
        let queue = AsyncPriorityQueue::new();
        let pulse = spawn_heartbeat(Arc::clone(&state), queue.clone());
        let busy = aura_core_base::busy::begin(CoreBusyActivityV1::DeletingSession);
        let shown = next_health(&queue, Duration::from_secs(15)).await;
        assert_eq!(shown["payload"]["busy"]["activity"], "deleting_session", "{shown}");
        drop(busy);
        // Other work of this test process may overlap for a moment; the busy word goes.
        let cleared = time::timeout(Duration::from_secs(20), async {
            loop {
                let next = next_health(&queue, Duration::from_secs(20)).await;
                if next["payload"].get("busy").is_none() {
                    return next;
                }
            }
        })
        .await
        .expect("the busy word goes when the work ends");
        assert!(
            cleared["revision"].as_str().unwrap_or("0").parse::<u64>().unwrap_or(0)
                > shown["revision"].as_str().unwrap_or("0").parse::<u64>().unwrap_or(0),
            "{cleared}"
        );
        pulse.abort();
    }
}
