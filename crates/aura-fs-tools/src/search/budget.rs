use std::sync::Mutex;

use super::policy::MAX_RESERVE_BYTES;

/// Why an allowance increase was or was not granted.
///
/// "Not your turn yet" and "the pool is empty" are different facts: the first
/// resolves as earlier candidates settle, the second does not. Reporting the
/// first as budget exhaustion would make the outcome depend on which worker
/// happened to ask first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Grow {
    /// Additional bytes were granted.
    Granted(u64),
    /// The budget still holds bytes, but they belong to an earlier ordinal that
    /// has not settled yet. The caller must wait for its turn, not skip the file.
    WaitingForPredecessor,
    /// No unreserved bytes remain even though this ordinal holds the turn.
    Exhausted,
}

/// Byte budget with ordinal-order reservation so a later worker cannot spend
/// bytes that an earlier candidate still needs.
///
/// Growth is granted in candidate order: only the ordinal currently holding the
/// turn (`next_turn`) may draw from the unreserved pool. The coordinator
/// advances the turn as ordinals settle, so a later file waits instead of being
/// denied when an earlier file is still being scanned.
pub(crate) struct ByteBudget {
    inner: Mutex<Inner>,
}

struct Inner {
    remaining_unreserved: u64,
    reserved: u64,
    consumed: u64,
    /// The lowest ordinal that has not settled yet; it holds the turn.
    next_turn: usize,
    /// Ordinals that have already settled, so the turn can advance past them.
    settled: std::collections::BTreeSet<usize>,
}

impl Inner {
    /// Move the turn past every ordinal that already settled.
    fn advance_turn(&mut self) {
        while self.settled.remove(&self.next_turn) {
            self.next_turn = self.next_turn.saturating_add(1);
        }
    }
}

impl ByteBudget {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                remaining_unreserved: super::policy::total_bytes(),
                reserved: 0,
                consumed: 0,
                next_turn: 0,
                settled: std::collections::BTreeSet::new(),
            }),
        }
    }

    /// Reserve for the next ordinal before dispatch. `hint` is logical size or
    /// `MAX_FILE_BYTES` when unknown. Reservation is a planning hint, not a
    /// turn: only [`Self::try_grow_for`] is ordered.
    pub(crate) fn try_reserve(&self, hint: u64) -> Option<u64> {
        let need = hint.clamp(1, MAX_RESERVE_BYTES);
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.remaining_unreserved < need {
            if inner.remaining_unreserved == 0 {
                return None;
            }
            let granted = inner.remaining_unreserved;
            inner.remaining_unreserved = 0;
            inner.reserved = inner.reserved.saturating_add(granted);
            return Some(granted);
        }
        inner.remaining_unreserved -= need;
        inner.reserved = inner.reserved.saturating_add(need);
        Some(need)
    }

    /// Settle one *attempt*: bill the bytes it actually read and return the rest
    /// of its allowance to the pool.
    ///
    /// This deliberately does not touch the ordinal order. An attempt can end
    /// while the same file is about to be retried, and an ordinal that is still
    /// being read must keep its place in the queue or the retry would be told
    /// "not your turn" forever.
    pub(crate) fn release_attempt(&self, used: u64, reserved: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let used = used.min(reserved);
        inner.consumed = inner.consumed.saturating_add(used);
        inner.reserved = inner.reserved.saturating_sub(reserved);
        if reserved > used {
            inner.remaining_unreserved = inner.remaining_unreserved.saturating_add(reserved - used);
        }
    }

    /// Finish one file ordinal: it has its final result, so the growth turn may
    /// move past it. Called exactly once per candidate on every terminal path.
    pub(crate) fn finish_ordinal(&self, ordinal: usize) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.settled.insert(ordinal);
        inner.advance_turn();
    }

    /// Ask for `extra` bytes for `ordinal`.
    pub(crate) fn try_grow_for(&self, ordinal: usize, extra: u64) -> Grow {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if ordinal != inner.next_turn {
            return Grow::WaitingForPredecessor;
        }
        if inner.remaining_unreserved == 0 {
            return Grow::Exhausted;
        }
        let granted = extra.min(inner.remaining_unreserved);
        inner.remaining_unreserved -= granted;
        inner.reserved = inner.reserved.saturating_add(granted);
        Grow::Granted(granted)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn remaining_unreserved(&self) -> u64 {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).remaining_unreserved
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn consumed(&self) -> u64 {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).consumed
    }
}

#[cfg(test)]
mod tests {
    use super::super::policy::{MAX_FILE_BYTES, total_bytes};
    use super::*;

    #[test]
    fn leftover_is_available_to_a_later_reserve() {
        let budget = ByteBudget::new();
        let first = budget.try_reserve(64).expect("first");
        budget.release_attempt(8, first);
        budget.finish_ordinal(0);
        assert_eq!(budget.consumed(), 8);
        let second = budget.try_reserve(MAX_FILE_BYTES).expect("leftover");
        assert_eq!(second, MAX_FILE_BYTES);
        assert_eq!(budget.remaining_unreserved(), total_bytes() - 8 - MAX_FILE_BYTES);
    }

    #[test]
    fn grow_cannot_exceed_unreserved() {
        let budget = ByteBudget::new();
        let mut held = Vec::new();
        while let Some(chunk) = budget.try_reserve(MAX_FILE_BYTES) {
            held.push(chunk);
        }
        assert_eq!(budget.try_grow_for(0, 16), Grow::Exhausted);
        let returned = held.pop().expect("held");
        budget.release_attempt(0, returned);
        budget.finish_ordinal(0);
        assert_eq!(budget.try_grow_for(1, 16), Grow::Granted(16));
        assert_eq!(budget.remaining_unreserved(), returned.saturating_sub(16));
    }

    #[test]
    fn consume_releases_try_grow_as_part_of_held() {
        let budget = ByteBudget::new();
        let reserved = budget.try_reserve(17).expect("reserve");
        let granted = budget.try_grow_for(0, 65_536);
        assert_eq!(granted, Grow::Granted(65_536));
        budget.release_attempt(17, reserved.saturating_add(65_536));
        budget.finish_ordinal(0);
        assert_eq!(budget.consumed(), 17);
        assert_eq!(budget.remaining_unreserved(), total_bytes() - 17);
    }

    #[test]
    fn a_later_ordinal_waits_instead_of_reporting_exhaustion() {
        // Ordinal 1 asks for more while ordinal 0 still holds the turn. That is
        // "not your turn yet", not "the budget is gone": ordinal 0's own
        // reservation may still be returned to the pool.
        let budget = ByteBudget::new();
        let held_by_zero = budget.try_reserve(MAX_FILE_BYTES).expect("reserve");
        assert_eq!(budget.try_grow_for(1, 1024), Grow::WaitingForPredecessor);
        // Ordinal 0 settles and returns almost everything it reserved.
        budget.release_attempt(8, held_by_zero);
        budget.finish_ordinal(0);
        assert_eq!(budget.try_grow_for(1, 1024), Grow::Granted(1024));
        budget.release_attempt(8, 1024);
        budget.finish_ordinal(1);
        assert_eq!(budget.consumed(), 16);
    }

    #[test]
    fn the_turn_advances_past_every_settled_ordinal() {
        let budget = ByteBudget::new();
        let first = budget.try_reserve(1024).expect("reserve");
        let second = budget.try_reserve(1024).expect("reserve");
        // Ordinals settle in candidate order; once 0 and 1 are done the turn
        // reaches 2, so a later candidate is not blocked behind finished work.
        assert_eq!(budget.try_grow_for(2, 64), Grow::WaitingForPredecessor);
        budget.release_attempt(1, first);
        budget.finish_ordinal(0);
        assert_eq!(budget.try_grow_for(2, 64), Grow::WaitingForPredecessor);
        budget.release_attempt(1, second);
        budget.finish_ordinal(1);
        assert_eq!(budget.try_grow_for(2, 64), Grow::Granted(64));
    }

    #[test]
    fn exhaustion_is_reported_only_when_the_turn_holder_cannot_grow() {
        let budget = ByteBudget::new();
        let mut held = Vec::new();
        while let Some(chunk) = budget.try_reserve(MAX_FILE_BYTES) {
            held.push(chunk);
        }
        // Ordinal 0 holds the turn and the pool is empty behind its own
        // reservations, so this is genuine exhaustion for the current turn.
        assert_eq!(budget.try_grow_for(0, 1), Grow::Exhausted);
        // A later ordinal is told to wait, not that the budget is gone.
        assert_eq!(budget.try_grow_for(1, 1), Grow::WaitingForPredecessor);
    }
}
