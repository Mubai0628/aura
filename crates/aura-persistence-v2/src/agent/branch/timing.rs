//! How long each part of making a branch
//! took, so a slow branch says where its time went in Core's log.
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct BranchTiming {
    /// Microseconds spent in each phase.
    pub phases: BTreeMap<&'static str, u128>,
    pub turns: usize,
    pub calls: usize,
}

impl BranchTiming {
    pub(super) fn time<T>(&mut self, phase: &'static str, work: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let value = work();
        *self.phases.entry(phase).or_default() += started.elapsed().as_micros();
        value
    }

    /// Milliseconds per phase, for the log.
    #[must_use]
    pub fn milliseconds(&self) -> BTreeMap<&'static str, u128> {
        self.phases.iter().map(|(phase, micros)| (*phase, micros / 1000)).collect()
    }
}
