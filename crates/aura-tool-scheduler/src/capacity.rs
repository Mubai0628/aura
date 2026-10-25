//! The single dynamic global ordinary-Tool capacity.
//!
//! One runtime owns exactly one instance, shared by every session, so the count
//! of executing ordinary Tools never exceeds the committed limit no matter how
//! many sessions or settings revisions are in play. The limit and the in-flight
//! count live under one lock, which makes admission, limit changes and wake-ups
//! linearize against each other instead of racing on independent reads.
//!
//! Lowering the limit never revokes a member that is already running: the count
//! stays above the new limit until real executions finish, and only then do new
//! callers get in. Raising the limit wakes every waiter immediately.
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, watch};

/// RAII membership. Capacity returns when the execution permit that holds this
/// slot is dropped, so an Unknown or quarantined execution keeps its slot for as
/// long as the underlying attempt is still held.
pub struct ToolCapacitySlot {
    capacity: Arc<ToolCapacityState>,
}

impl Drop for ToolCapacitySlot {
    fn drop(&mut self) {
        if let Ok(mut state) = self.capacity.state.lock() {
            state.in_flight = state.in_flight.saturating_sub(1);
        }
        self.capacity.released.notify_waiters();
    }
}

#[derive(Debug)]
struct CapacityState {
    limit: u32,
    in_flight: u32,
}

/// The shared state behind one runtime's global ordinary-Tool capacity. Cloning
/// the handle shares the same mutex and notifier; it never forks the count.
#[derive(Clone)]
pub struct ToolCapacity {
    state: Arc<ToolCapacityState>,
}

struct ToolCapacityState {
    state: Mutex<CapacityState>,
    released: Notify,
}

impl ToolCapacity {
    /// The limit must already be validated by the caller against the frozen
    /// contract range.
    #[must_use]
    pub fn new(limit: u32) -> Self {
        Self {
            state: Arc::new(ToolCapacityState {
                state: Mutex::new(CapacityState { limit, in_flight: 0 }),
                released: Notify::new(),
            }),
        }
    }

    /// Applies a committed settings value. A raised limit admits waiters; a
    /// lowered limit lets current members finish and pauses new admission.
    pub fn set_limit(&self, limit: u32) {
        if let Ok(mut state) = self.state.state.lock() {
            state.limit = limit;
        }
        self.state.released.notify_waiters();
    }

    #[must_use]
    pub fn limit(&self) -> u32 {
        self.state.state.lock().map_or(0, |state| state.limit)
    }

    #[must_use]
    pub fn in_flight(&self) -> u32 {
        self.state.state.lock().map_or(0, |state| state.in_flight)
    }

    /// Waits for one global slot.
    ///
    /// Cancellation is checked before the claim, so an already-cancelled call
    /// never consumes a slot even when capacity is free. A closed cancellation
    /// channel is treated as a stop request rather than an infrastructure fault.
    pub async fn acquire(
        &self,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Option<ToolCapacitySlot> {
        loop {
            if *cancellation.borrow() {
                return None;
            }
            // Register interest before the state read, so a release or a limit
            // change happening in between cannot be missed.
            let waiter = self.state.released.notified();
            let claimed = match self.state.state.lock() {
                Ok(mut state) => {
                    if state.in_flight < state.limit {
                        state.in_flight = state.in_flight.saturating_add(1);
                        true
                    } else {
                        false
                    }
                }
                Err(_) => return None,
            };
            if claimed {
                return Some(ToolCapacitySlot { capacity: Arc::clone(&self.state) });
            }
            tokio::select! {
                biased;
                changed = cancellation.changed() => {
                    if changed.is_err() || *cancellation.borrow() {
                        return None;
                    }
                }
                () = waiter => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::watch;

    use super::ToolCapacity;

    fn channel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
        watch::channel(false)
    }

    #[tokio::test]
    async fn a_cancelled_call_never_takes_a_free_slot() {
        let capacity = ToolCapacity::new(4);
        let (sender, mut receiver) = channel();
        sender.send_replace(true);
        assert!(capacity.acquire(&mut receiver).await.is_none());
        assert_eq!(capacity.in_flight(), 0, "a cancelled call must not consume capacity");
    }

    #[tokio::test]
    async fn a_closed_cancellation_channel_stops_the_wait_without_a_slot() {
        let capacity = ToolCapacity::new(1);
        let held = {
            let (_sender, mut receiver) = channel();
            capacity.acquire(&mut receiver).await.expect("held")
        };
        let (sender, mut receiver) = channel();
        drop(sender);
        // Capacity is exhausted, so this wait can only end because the channel
        // closed. A closed channel is a stop request, never a slot.
        assert!(capacity.acquire(&mut receiver).await.is_none());
        assert_eq!(capacity.in_flight(), 1);
        drop(held);
    }

    #[tokio::test]
    async fn admission_stops_at_the_limit_and_resumes_when_a_member_releases() {
        let capacity = ToolCapacity::new(2);
        let (_, mut receiver) = channel();
        let first = capacity.acquire(&mut receiver).await.expect("first");
        let second = capacity.acquire(&mut receiver).await.expect("second");
        assert_eq!(capacity.in_flight(), 2);

        // The sender must outlive the wait, otherwise the channel closes and the
        // wait stops for the wrong reason.
        let (_sender, mut waiting) = channel();
        let blocked = capacity.clone();
        let attempt = tokio::spawn(async move { blocked.acquire(&mut waiting).await });
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(!attempt.is_finished(), "a full capacity must not admit an extra member");

        drop(first);
        let admitted = tokio::time::timeout(Duration::from_millis(500), attempt)
            .await
            .expect("a release must wake the waiter")
            .expect("join");
        assert!(admitted.is_some());
        assert_eq!(capacity.in_flight(), 2);
        drop(admitted);
        drop(second);
        assert_eq!(capacity.in_flight(), 0);
    }

    #[tokio::test]
    async fn lowering_the_limit_keeps_members_and_pauses_admission_until_it_frees() {
        let capacity = ToolCapacity::new(2);
        let (_, mut receiver) = channel();
        let first = capacity.acquire(&mut receiver).await.expect("first");
        let second = capacity.acquire(&mut receiver).await.expect("second");

        capacity.set_limit(1);
        let (_sender, mut waiting) = channel();
        let blocked = capacity.clone();
        let attempt = tokio::spawn(async move { blocked.acquire(&mut waiting).await });
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(
            !attempt.is_finished(),
            "one member still holds the single allowed slot, so nothing new may enter"
        );

        // Releasing down to exactly the new limit is still no free capacity: one
        // member remains, so admission stays paused.
        drop(first);
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(
            !attempt.is_finished(),
            "one surviving member already fills the lowered limit, so nobody else may enter"
        );
        assert_eq!(capacity.in_flight(), 1);

        // Only a real release below the limit admits the waiter.
        drop(second);
        let admitted = tokio::time::timeout(Duration::from_millis(500), attempt)
            .await
            .expect("a release below the limit admits exactly one member")
            .expect("join");
        assert!(admitted.is_some());
        assert_eq!(capacity.in_flight(), 1);
        drop(admitted);
        assert_eq!(capacity.in_flight(), 0);
    }

    #[tokio::test]
    async fn raising_the_limit_wakes_every_waiter() {
        let capacity = ToolCapacity::new(1);
        let (_, mut receiver) = channel();
        let held = capacity.acquire(&mut receiver).await.expect("held");
        // One shared channel whose sender stays alive for the whole test.
        let (sender, waiting) = channel();
        let mut attempts = Vec::new();
        for _ in 0..3 {
            let clone = capacity.clone();
            let mut waiting = waiting.clone();
            attempts.push(tokio::spawn(async move { clone.acquire(&mut waiting).await }));
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
        capacity.set_limit(4);
        let mut admitted = Vec::new();
        for attempt in attempts {
            let slot = tokio::time::timeout(Duration::from_millis(500), attempt)
                .await
                .expect("raised limit admits the waiter")
                .expect("join");
            assert!(slot.is_some());
            admitted.push(slot);
        }
        assert_eq!(capacity.in_flight(), 4);
        drop(admitted);
        drop(held);
        drop(sender);
    }

    /// A wait with capacity still free returns immediately and without waiting
    /// for any event, so a later limit change cannot be what admitted it.
    #[tokio::test]
    async fn a_wait_completes_from_real_events_only() {
        let capacity = ToolCapacity::new(2);
        let (sender, mut receiver) = channel();
        let first = capacity.acquire(&mut receiver).await.expect("first");
        let second = capacity.acquire(&mut receiver).await.expect("second");
        assert_eq!(capacity.in_flight(), 2);

        let (_waiter_sender, mut waiting) = channel();
        let blocked = capacity.clone();
        let attempt = tokio::spawn(async move { blocked.acquire(&mut waiting).await });
        // No event at all: the waiter must stay parked rather than time out into
        // a slot.
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(!attempt.is_finished(), "a full capacity parks the waiter indefinitely");

        // A limit raise that is still below two members also admits nobody.
        capacity.set_limit(2);
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(!attempt.is_finished(), "a limit equal to the member count admits nobody");

        capacity.set_limit(3);
        let admitted = tokio::time::timeout(Duration::from_millis(500), attempt)
            .await
            .expect("the raise wakes the waiter")
            .expect("join");
        assert!(admitted.is_some());
        assert_eq!(capacity.in_flight(), 3);
        drop(admitted);
        drop(second);
        drop(first);
        drop(sender);
    }
}
