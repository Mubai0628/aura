//! Candidate background ownership is keyed by the recorded Job, never by executable name.
use super::{CompatExecution, JobObservation, StopObservation};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, time::Duration};

#[derive(Default)]
pub struct CompatJobs {
    jobs: BTreeMap<String, CompatExecution>,
}

impl CompatJobs {
    pub fn register(&mut self, mut execution: CompatExecution) -> Result<String> {
        ensure!(self.jobs.len() < 32, "compat background capacity reached");
        ensure!(execution.observe()?.active_processes > 0, "empty Job is not background work");
        let id = execution.receipt().nonce;
        ensure!(!self.jobs.contains_key(&id), "duplicate background Job");
        self.jobs.insert(id.clone(), execution);
        Ok(id)
    }
    pub fn len(&self) -> usize {
        self.jobs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
    pub fn observe(&mut self, id: &str) -> Result<JobObservation> {
        self.jobs.get_mut(id).ok_or_else(|| anyhow::anyhow!("unknown owned Job"))?.observe()
    }
    pub fn stop(&mut self, id: &str, bound: Duration) -> Result<StopObservation> {
        let stopped =
            self.jobs.get_mut(id).ok_or_else(|| anyhow::anyhow!("unknown owned Job"))?.stop(bound);
        if stopped.confirmed {
            self.jobs.remove(id);
        }
        Ok(stopped)
    }
}
