//! The watchdog's clock: how much time each pass is told has gone by since the last one.
use super::{TICK, watch};
use aura_agent_team::{TeamCall, TeamFuture, TeamPort, TeamTools, WatchFuture};
use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;

/// Hands on what each pass is told; the first pass is held up for `held`, as a busy store or a
/// machine waking from sleep holds the clock up.
struct Passes {
    seen: mpsc::UnboundedSender<Duration>,
    held: Duration,
    first: AtomicBool,
}

impl TeamTools for Passes {
    fn call(self: Arc<Self>, _call: TeamCall) -> TeamFuture {
        Box::pin(async { Err("this team takes no calls".to_owned()) })
    }

    fn watch(self: Arc<Self>, elapsed: Duration) -> WatchFuture {
        Box::pin(async move {
            if self.seen.send(elapsed).is_err() {
                return;
            }
            if self.first.swap(false, Ordering::SeqCst) {
                tokio::time::sleep(self.held).await;
            }
        })
    }
}

async fn next(seen: &mut mpsc::UnboundedReceiver<Duration>) -> Duration {
    tokio::time::timeout(Duration::from_secs(10), seen.recv())
        .await
        .ok()
        .flatten()
        .expect("the watchdog makes another pass")
}

#[tokio::test]
async fn a_gap_far_past_the_tick_counts_as_three_ticks_and_the_clock_stops_with_its_team() {
    let (sender, mut seen) = mpsc::unbounded_channel();
    let passes = Arc::new(Passes { seen: sender, held: TICK * 10, first: AtomicBool::new(true) });
    let port = Arc::new(TeamPort::default());
    let bound: Weak<Passes> = Arc::downgrade(&passes);
    port.bind(bound);
    let clock = tokio::spawn(watch(Arc::downgrade(&port)));

    let first = next(&mut seen).await;
    assert!(first >= TICK, "a pass counts the tick it waited: {first:?}");
    let after_the_gap = next(&mut seen).await;
    assert_eq!(after_the_gap, TICK * 3, "eleven ticks without a pass count as three");
    let ordinary = next(&mut seen).await;
    assert!(
        (TICK..=TICK * 3).contains(&ordinary),
        "an ordinary pass counts its own tick: {ordinary:?}"
    );

    drop(port);
    tokio::time::timeout(Duration::from_secs(10), clock)
        .await
        .expect("the clock stops once its team is gone")
        .expect("the clock ends without a panic");
}
