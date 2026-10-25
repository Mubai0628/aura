//! Model-owned dynamic membership; fixed tools are a separate user authority.
use aura_domain_common::ToolId;
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionMembershipError {
    #[error("tools.fixed_tool: fixed tools are user-managed and cannot be unloaded or replaced")]
    FixedTool,
    #[error("tools.unknown_tool_id: search available tools before loading")]
    UnknownTool,
    #[error(
        "tools.capacity_exceeded: explicitly unload or replace dynamic tools within max_dynamic_tool_schemas; fixed tools do not count"
    )]
    CapacityExceeded,
}

pub fn update_session_membership(
    before: &BTreeSet<ToolId>,
    fixed: &BTreeSet<ToolId>,
    available: &BTreeSet<ToolId>,
    input: &aura_tool_contract::AgentToolLoadInput,
    limit: u32,
) -> Result<BTreeSet<ToolId>, SessionMembershipError> {
    let mut next = before.clone();
    for id in &input.replace {
        if fixed.contains(id) {
            return Err(SessionMembershipError::FixedTool);
        }
        next.remove(id);
    }
    for id in &input.tool_ids {
        if !available.contains(id) {
            return Err(SessionMembershipError::UnknownTool);
        }
        if !fixed.contains(id) {
            next.insert(*id);
        }
    }
    if next.difference(before).next().is_some() && next.difference(fixed).count() > limit as usize {
        return Err(SessionMembershipError::CapacityExceeded);
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_tool_contract::AgentToolLoadInput;
    #[test]
    fn cap_replacement_lowering_and_unavailable_member_removal_are_explicit() {
        let [a, b, c, fixed] = [1, 2, 3, 4].map(|v| ToolId::from_bytes([v; 16]));
        let available = BTreeSet::from([a, b, c, fixed]);
        let fixed = BTreeSet::from([fixed]);
        let before = BTreeSet::from([a, b]);
        let update = |load: Vec<ToolId>, replace: Vec<ToolId>, limit| {
            update_session_membership(
                &before,
                &fixed,
                &available,
                &AgentToolLoadInput { tool_ids: load, replace },
                limit,
            )
        };
        assert_eq!(update(vec![c], vec![], 2), Err(SessionMembershipError::CapacityExceeded));
        assert_eq!(update(vec![c], vec![a], 2), Ok(BTreeSet::from([b, c])));
        assert_eq!(update(vec![], vec![], 1), Ok(before.clone()), "lowering cap does not evict");
        assert_eq!(update(vec![c], vec![a], 1), Err(SessionMembershipError::CapacityExceeded));
        assert_eq!(update(vec![c], vec![a, b], 1), Ok(BTreeSet::from([c])));
        assert_eq!(update(vec![*fixed.first().expect("fixed")], vec![], 2), Ok(before.clone()));
        assert_eq!(
            update(vec![], fixed.iter().copied().collect(), 2),
            Err(SessionMembershipError::FixedTool)
        );
        assert_eq!(
            update(vec![ToolId::new()], vec![], 50),
            Err(SessionMembershipError::UnknownTool)
        );
        assert_eq!(
            update_session_membership(
                &before,
                &fixed,
                &BTreeSet::new(),
                &AgentToolLoadInput { tool_ids: vec![], replace: vec![a] },
                1
            ),
            Ok(BTreeSet::from([b]))
        );
    }
}
