//! Process groups: the commands of one session that Core gives a [`ProcessPolicy`] run in
//! execution Jobs nested in that session's group Job, and every such group is nested in one Job
//! for all of them. Each group also gets its own temporary folder and port range.
use anyhow::{Result, bail};
use aura_windows_platform::{GroupJob, GroupLimits, NativeEnvironmentEntry};
use std::{
    collections::HashMap,
    ops::RangeInclusive,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// What one session's commands run under, as Core decides it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessPolicy {
    /// Commands with the same key share one group Job: its limits are theirs together.
    pub group: String,
    pub limits: GroupLimits,
    /// Hard cap, in hundredths of a percent of all processors, of every group's processes
    /// together. The newest policy's value holds, for the processes already running too.
    pub all_cpu_rate: u32,
    /// The group's TEMP and TMP.
    pub temp_directory: PathBuf,
}

/// Below the dynamic port range Windows hands out for outgoing connections (49152 and up).
const PORT_BASE: u16 = 30_000;
const PORTS_PER_GROUP: u16 = 100;
/// Groups held at once; at the limit, groups with no live process are let go first.
const GROUPS_MAX: u16 = 64;

pub(crate) struct Group {
    job: GroupJob,
    slot: u16,
}

impl Group {
    pub(crate) fn job(&self) -> &GroupJob {
        &self.job
    }

    pub(crate) fn ports(&self) -> RangeInclusive<u16> {
        let first = PORT_BASE + self.slot * PORTS_PER_GROUP;
        first..=first + PORTS_PER_GROUP - 1
    }
}

#[derive(Default)]
pub(crate) struct ProcessGroups {
    /// The Job every group is nested in, and the CPU cap last set on it.
    all: Mutex<Option<(Arc<GroupJob>, u32)>>,
    groups: Mutex<HashMap<String, Arc<Group>>>,
}

impl ProcessGroups {
    /// The Job for all groups and the policy's own group, created on first use.
    pub(crate) fn enter(&self, policy: &ProcessPolicy) -> Result<(Arc<GroupJob>, Arc<Group>)> {
        let all = {
            let mut all = self.all.lock().map_err(|_| anyhow::anyhow!("compat.groups_lock"))?;
            match all.as_mut() {
                Some((job, rate)) => {
                    if *rate != policy.all_cpu_rate {
                        job.set_cpu_rate(policy.all_cpu_rate)?;
                        *rate = policy.all_cpu_rate;
                    }
                    Arc::clone(job)
                }
                None => {
                    let limits = GroupLimits { cpu_rate: policy.all_cpu_rate, memory_bytes: None };
                    let job = Arc::new(GroupJob::create(limits)?);
                    *all = Some((Arc::clone(&job), policy.all_cpu_rate));
                    job
                }
            }
        };
        let mut groups = self.groups.lock().map_err(|_| anyhow::anyhow!("compat.groups_lock"))?;
        if let Some(group) = groups.get(&policy.group) {
            return Ok((all, Arc::clone(group)));
        }
        if groups.len() >= usize::from(GROUPS_MAX) {
            // A group whose Job is empty holds nothing a later command could still need.
            groups.retain(|_, group| group.job.active_processes().map_or(true, |live| live > 0));
        }
        let Some(slot) = (0..GROUPS_MAX).find(|slot| groups.values().all(|g| g.slot != *slot))
        else {
            bail!("compat.process_groups_full");
        };
        let group = Arc::new(Group { job: GroupJob::create(policy.limits)?, slot });
        groups.insert(policy.group.clone(), Arc::clone(&group));
        Ok((all, group))
    }

    #[cfg(test)]
    pub(crate) fn group(&self, key: &str) -> Option<Arc<Group>> {
        self.groups.lock().ok()?.get(key).cloned()
    }
}

/// The group's surroundings in a command's environment: its own TEMP and TMP and its port range
/// (`AURA_PORT_RANGE`), and `PORT` set to the range's first port when the user's own
/// environment sets none.
pub(crate) fn surround(
    entries: &mut Vec<NativeEnvironmentEntry>,
    temp: &Path,
    ports: &RangeInclusive<u16>,
) -> Result<()> {
    let Some(temp) = temp.to_str() else { bail!("compat.temp_directory_not_representable") };
    super::environment::replace(entries, "TEMP", temp);
    super::environment::replace(entries, "TMP", temp);
    let range = format!("{}-{}", ports.start(), ports.end());
    super::environment::replace(entries, "AURA_PORT_RANGE", &range);
    if !entries.iter().any(|entry| entry.name.eq_ignore_ascii_case("PORT")) {
        super::environment::replace(entries, "PORT", &ports.start().to_string());
    }
    Ok(())
}

#[cfg(test)]
#[path = "groups_tests.rs"]
mod tests;
