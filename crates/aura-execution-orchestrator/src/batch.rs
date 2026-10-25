//! Scheduling of an already admitted immutable DAG. Core commits each atomic
//! unit through its owning transaction, then records the committed results here.
use aura_domain_common::ToolCallId;
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::{
    ToolExecutionPlanV1, ToolNotExecutedReasonV1, ToolResultOutcomeV1, ToolResultV1,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchUnit {
    /// Topological order, including control dependencies inside the group.
    pub calls: Vec<ToolCallId>,
    pub atomic_group: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitReadiness {
    Ready(DispatchUnit),
    Rejected { unit: DispatchUnit, reason: ToolNotExecutedReasonV1 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Pending,
    Running,
    Succeeded,
    Blocked,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("batch progress does not match the frozen dependency/atomic-group plan")]
pub struct BatchProgressError;

pub struct BatchProgress {
    plan: ToolExecutionPlanV1,
    units: Vec<DispatchUnit>,
    state: BTreeMap<ToolCallId, State>,
}
impl BatchProgress {
    pub fn new(
        plan: ToolExecutionPlanV1,
        committed: &[ToolResultV1],
    ) -> Result<Self, BatchProgressError> {
        plan.validate().map_err(|_| BatchProgressError)?;
        let mut groups = BTreeMap::<u32, Vec<ToolCallId>>::new();
        let mut units = Vec::new();
        for call in &plan.calls {
            if let Some(group) = call.execution_atomic_group {
                groups.entry(group).or_default().push(call.tool_call_id);
            } else {
                units.push(DispatchUnit { calls: vec![call.tool_call_id], atomic_group: None });
            }
        }
        for (group, calls) in groups {
            let mut pending: BTreeSet<_> = calls.into_iter().collect();
            let mut ordered = Vec::new();
            while !pending.is_empty() {
                let next = plan
                    .calls
                    .iter()
                    .find(|c| {
                        pending.contains(&c.tool_call_id)
                            && c.dependency_tool_call_ids.iter().all(|d| !pending.contains(d))
                    })
                    .ok_or(BatchProgressError)?;
                pending.remove(&next.tool_call_id);
                ordered.push(next.tool_call_id);
            }
            units.push(DispatchUnit { calls: ordered, atomic_group: Some(group) });
        }
        units.sort_by_key(|u| {
            plan.calls.iter().find(|c| c.tool_call_id == u.calls[0]).map(|c| c.ordinal)
        });
        let state = plan.calls.iter().map(|c| (c.tool_call_id, State::Pending)).collect();
        let mut value = Self { plan, units, state };
        let mut seen = BTreeSet::new();
        for result in committed {
            if !seen.insert(result.tool_call_id) {
                return Err(BatchProgressError);
            }
            if let Some(state) = value.state.get_mut(&result.tool_call_id) {
                *state = result_state(result)?;
            }
            // Preflight rejection calls deliberately do not enter the plan.
            else if !matches!(result.outcome, ToolResultOutcomeV1::NotExecuted { .. }) {
                return Err(BatchProgressError);
            }
        }
        for unit in &value.units {
            let n = unit.calls.iter().filter(|c| seen.contains(c)).count();
            if n != 0 && n != unit.calls.len() {
                return Err(BatchProgressError);
            }
        }
        // Collapsing a group must not introduce a cycle through another unit.
        let mut remaining = value.units.clone();
        while !remaining.is_empty() {
            let active: BTreeSet<_> =
                remaining.iter().flat_map(|u| u.calls.iter().copied()).collect();
            let index = remaining
                .iter()
                .position(|u| value.external_dependencies(u).iter().all(|d| !active.contains(d)))
                .ok_or(BatchProgressError)?;
            remaining.remove(index);
        }
        Ok(value)
    }
    pub fn ready(&self, cancelled: bool) -> Vec<UnitReadiness> {
        self.units
            .iter()
            .filter(|u| u.calls.iter().all(|c| self.state[c] == State::Pending))
            .filter_map(|unit| {
                let dependencies = self.external_dependencies(unit);
                let reason = if cancelled {
                    Some(ToolNotExecutedReasonV1::CancelledBeforeDispatch)
                } else if dependencies.iter().any(|d| self.state[d] == State::Blocked) {
                    Some(ToolNotExecutedReasonV1::DependencyNotExecuted)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    return Some(UnitReadiness::Rejected { unit: unit.clone(), reason });
                }
                dependencies
                    .iter()
                    .all(|d| self.state[d] == State::Succeeded)
                    .then(|| UnitReadiness::Ready(unit.clone()))
            })
            .collect()
    }
    pub fn start(&mut self, unit: &DispatchUnit) -> Result<(), BatchProgressError> {
        if !self
            .ready(false)
            .iter()
            .any(|v| matches!(v, UnitReadiness::Ready(ready) if ready == unit))
        {
            return Err(BatchProgressError);
        }
        for id in &unit.calls {
            self.state.insert(*id, State::Running);
        }
        Ok(())
    }
    /// Call only after the owning Store has atomically committed every member.
    pub fn committed(
        &mut self,
        unit: &DispatchUnit,
        results: &[ToolResultV1],
    ) -> Result<(), BatchProgressError> {
        if !self.units.contains(unit) || results.len() != unit.calls.len() {
            return Err(BatchProgressError);
        }
        let mut next = Vec::new();
        for (id, result) in unit.calls.iter().zip(results) {
            if *id != result.tool_call_id
                || !matches!(self.state[id], State::Pending | State::Running)
            {
                return Err(BatchProgressError);
            }
            if self.state[id] == State::Pending
                && !matches!(result.outcome, ToolResultOutcomeV1::NotExecuted { .. })
            {
                return Err(BatchProgressError);
            }
            next.push((*id, result_state(result)?));
        }
        for (id, state) in next {
            self.state.insert(id, state);
        }
        Ok(())
    }
    pub fn finished(&self) -> bool {
        self.state.values().all(|v| matches!(v, State::Succeeded | State::Blocked))
    }
    fn external_dependencies(&self, unit: &DispatchUnit) -> BTreeSet<ToolCallId> {
        self.plan
            .calls
            .iter()
            .filter(|c| unit.calls.contains(&c.tool_call_id))
            .flat_map(|c| c.dependency_tool_call_ids.iter().copied())
            .filter(|d| !unit.calls.contains(d))
            .collect()
    }
}
fn result_state(result: &ToolResultV1) -> Result<State, BatchProgressError> {
    result.validate().map_err(|_| BatchProgressError)?;
    Ok(
        if matches!(result.outcome, ToolResultOutcomeV1::Succeeded)
            && result.side_effect_state != SideEffectStateV1::Unknown
        {
            State::Succeeded
        } else {
            State::Blocked
        },
    )
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
