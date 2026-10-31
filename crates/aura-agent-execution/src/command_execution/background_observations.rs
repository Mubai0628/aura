//! Bounded information-only sequence memory. Never blocks an execution callback.
use crate::agent_activity::{BackgroundObservationDisposition, background_observation_disposition};
use aura_domain_common::SecurityOperationId;
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const CAPACITY: usize = 512;

#[derive(Clone, Copy)]
struct Entry {
    sequence: u64,
    terminated: bool,
}

#[derive(Default)]
pub struct BackgroundObservations {
    entries: Mutex<BTreeMap<SecurityOperationId, Entry>>,
    missed: AtomicBool,
}

impl BackgroundObservations {
    pub(super) fn apply(
        &self,
        operation: SecurityOperationId,
        sequence: u64,
        terminal: bool,
    ) -> Option<BackgroundObservationDisposition> {
        if self.missed.load(Ordering::Acquire) {
            return None;
        }
        let Ok(mut entries) = self.entries.try_lock() else {
            self.missed.store(true, Ordering::Release);
            return None;
        };
        let entry = entries.get(&operation).copied();
        if entry.is_none() && entries.len() >= CAPACITY {
            // Retain terminal tombstones. Evicting them could resurrect a finished worker.
            self.missed.store(true, Ordering::Release);
            return None;
        }
        let disposition = background_observation_disposition(
            sequence,
            entry.map(|entry| entry.sequence),
            entry.is_some_and(|entry| entry.terminated),
        );
        if disposition == BackgroundObservationDisposition::Publish {
            entries.insert(operation, Entry { sequence, terminated: terminal });
        }
        Some(disposition)
    }

    /// Returns true when a worker disappeared without a terminal observation for a known row.
    pub(crate) fn finish_worker(&self, operation: SecurityOperationId) -> bool {
        let Ok(mut entries) = self.entries.try_lock() else {
            self.missed.store(true, Ordering::Release);
            return true;
        };
        if let Some(entry) = entries.remove(&operation) {
            // The actual worker has exited, so it can issue no more port callbacks. Until this
            // point its terminal tombstone rejects late messages; afterwards retain no history.
            !entry.terminated
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_information_memory_never_blocks_and_cannot_resurrect_a_row() {
        let state = BackgroundObservations::default();
        let operation = SecurityOperationId::new();
        assert_eq!(
            state.apply(operation, 1, false),
            Some(BackgroundObservationDisposition::Publish)
        );
        let held = state.entries.lock().expect("test hold");
        assert_eq!(state.apply(operation, 2, true), None);
        drop(held);
        assert_eq!(state.apply(operation, 3, false), None);
    }
    #[test]
    fn cache_is_bounded_and_retains_finished_tombstones() {
        let state = BackgroundObservations::default();
        let operation = SecurityOperationId::new();
        assert_eq!(
            state.apply(operation, 1, true),
            Some(BackgroundObservationDisposition::Publish)
        );
        assert_eq!(
            state.apply(operation, 2, false),
            Some(BackgroundObservationDisposition::AfterTerminal)
        );
        for _ in 1..CAPACITY {
            let _ = state.apply(SecurityOperationId::new(), 1, true);
        }
        assert_eq!(state.apply(SecurityOperationId::new(), 1, false), None);
        assert_eq!(state.entries.lock().expect("inspect bound").len(), CAPACITY);
    }
    #[test]
    fn worker_cleanup_does_not_confuse_normal_completion_with_unknown() {
        let state = BackgroundObservations::default();
        let normal = SecurityOperationId::new();
        let unknown = SecurityOperationId::new();
        let _ = state.apply(normal, 1, true);
        let _ = state.apply(unknown, 1, false);
        assert!(!state.finish_worker(normal));
        assert!(state.finish_worker(unknown));
        assert!(!state.finish_worker(unknown));
        assert!(state.entries.lock().expect("worker-owned cleanup").is_empty());
    }
}
