//! The cancellation intent that reaches a **not-yet-started resource wait**.
//!
//! # The fact this exists for
//!
//! A request that cannot be admitted because another execution holds its resource has **not
//! started**: no journal row, no side effect, no decision taken. That is exactly the state in which
//! a cancellation is meaningful and cheap — the request can simply not run — and it is also exactly
//! the state a cancellation could not previously reach. `aura.cancel` recorded its intent in the
//! connection ledger and cancelled a *search*; a file request parked in a resource wait had nothing
//! that observed the intent, so when the holder released, the waiter wrote anyway.
//!
//! The requirement is precise about the boundary, and this module is built to it:
//!
//! * an intent recorded while the request is **waiting for a resource** stops it from writing: it
//!   never starts, and it reports cancelled rather than a resource error;
//! * an intent recorded **after** the effect began is a different thing entirely and is **not**
//!   handled here. An already-started write keeps its owned completion, its `Unknown` possibility
//!   and its recovery responsibility — the file layer's own journal and settlement own that, and
//!   pretending a cancellation can undo a write in flight is the failure this boundary avoids.
//!
//! # Why it is keyed by correlation id and is not a second ledger
//!
//! The correlation id is the identity the connection, the ledger and every entry already agree on,
//! so this is a *waiter-side* view of an intent the ledger already recorded — not a competing source
//! of truth. It holds nothing but a set of "do not start this" marks, each removed as soon as its
//! request stops waiting, so a long session does not accumulate one entry per cancellation.
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
};

use aura_domain_common::CorrelationId;

/// The process-wide set of requests whose cancellation was requested while they waited.
fn cancelled_waits() -> &'static Mutex<HashSet<CorrelationId>> {
    static CANCELLED: OnceLock<Mutex<HashSet<CorrelationId>>> = OnceLock::new();
    CANCELLED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Records that a cancellation intent arrived for `request_id`.
///
/// Called from the real `aura.cancel` route, after the ledger has recorded the intent. Recording is
/// idempotent: two cancellations for one request are one fact.
pub fn request_cancel(request_id: CorrelationId) {
    if let Ok(mut cancelled) = cancelled_waits().lock() {
        cancelled.insert(request_id);
    }
}

/// Whether a cancellation intent has already been recorded for `request_id`.
///
/// This is what a waiter asks **at every point where it could still start**, including immediately
/// before it would take the resource, so an intent that arrived during the wait is never missed by
/// a poll boundary.
#[must_use]
pub fn cancellation_requested(request_id: CorrelationId) -> bool {
    cancelled_waits().lock().is_ok_and(|cancelled| cancelled.contains(&request_id))
}

/// Forgets one request's intent.
///
/// Called when the request stops waiting — whether it was cancelled, it gave up on its budget, or it
/// finally started. A request that has started is no longer a wait, and leaving its mark behind would
/// make a later request that reuses the correlation id look cancelled before it began.
pub fn clear_cancel(request_id: CorrelationId) {
    if let Ok(mut cancelled) = cancelled_waits().lock() {
        cancelled.remove(&request_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_intent_is_recorded_observed_and_cleared() {
        let id = CorrelationId::new();
        assert!(!cancellation_requested(id), "nothing was requested yet");
        request_cancel(id);
        assert!(cancellation_requested(id), "the intent is observable by a waiter");
        // A second intent for one request is the same fact.
        request_cancel(id);
        assert!(cancellation_requested(id));
        clear_cancel(id);
        assert!(!cancellation_requested(id), "a request that stopped waiting holds no live intent");
    }

    #[test]
    fn one_requests_intent_does_not_affect_another() {
        let mine = CorrelationId::new();
        let theirs = CorrelationId::new();
        request_cancel(mine);
        assert!(cancellation_requested(mine));
        assert!(
            !cancellation_requested(theirs),
            "a cancellation is about one request, never about the process"
        );
        clear_cancel(mine);
    }
}
