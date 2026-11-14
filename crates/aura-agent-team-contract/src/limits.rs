//! What the user allows the sub-agents: how many of one conversation work at once, the tokens and
//! the time each may use, and the share of the processors their commands take together.
use crate::Refusal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;

/// The limits under 设置 → 智能能力 → 多 Agent; they hold for every conversation.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamLimits {
    /// Sub-agents of one conversation working at once. One waiting for an approval does not
    /// count.
    pub max_running: u32,
    /// Tokens one sub-agent's answers may use in all, as their providers counted them; none
    /// when they are not limited.
    pub token_budget: Option<u64>,
    /// Minutes one sub-agent may work on what it was last given, not counting the time it waits
    /// for an approval.
    pub minutes_per_task: u32,
    /// Share of all processors, in percent, that every sub-agent's commands take together.
    pub cpu_percent: u32,
}

impl AgentTeamLimits {
    pub const DEFAULT: Self =
        Self { max_running: 3, token_budget: None, minutes_per_task: 120, cpu_percent: 75 };
    pub const RUNNING: RangeInclusive<u32> = 1..=8;
    pub const TOKENS: RangeInclusive<u64> = 100_000..=100_000_000;
    pub const MINUTES: RangeInclusive<u32> = 5..=480;
    pub const CPU: RangeInclusive<u32> = 10..=100;
    /// The share of the token budget past which the main agent is told.
    pub const WARNING_PERCENT: u64 = 80;
    /// The smallest of each limit the settings take.
    pub const MINIMUM: Self = Self {
        max_running: *Self::RUNNING.start(),
        token_budget: Some(*Self::TOKENS.start()),
        minutes_per_task: *Self::MINUTES.start(),
        cpu_percent: *Self::CPU.start(),
    };
    /// The largest of each limit the settings take.
    pub const MAXIMUM: Self = Self {
        max_running: *Self::RUNNING.end(),
        token_budget: Some(*Self::TOKENS.end()),
        minutes_per_task: *Self::MINUTES.end(),
        cpu_percent: *Self::CPU.end(),
    };

    pub fn check(&self) -> Result<(), Refusal> {
        let refused = |message: String| Err(Refusal::new("agents.limits_invalid", message));
        if !Self::RUNNING.contains(&self.max_running) {
            return refused(format!(
                "同时运行的子 Agent 要在 {} 到 {} 个之间。",
                Self::RUNNING.start(),
                Self::RUNNING.end()
            ));
        }
        if let Some(budget) = self.token_budget
            && !Self::TOKENS.contains(&budget)
        {
            return refused(format!(
                "每个子 Agent 的 token 上限要在 {} 到 {} 之间。",
                Self::TOKENS.start(),
                Self::TOKENS.end()
            ));
        }
        if !Self::MINUTES.contains(&self.minutes_per_task) {
            return refused(format!(
                "每个任务的用时上限要在 {} 到 {} 分钟之间。",
                Self::MINUTES.start(),
                Self::MINUTES.end()
            ));
        }
        if !Self::CPU.contains(&self.cpu_percent) {
            return refused(format!(
                "子 Agent 命令合计的 CPU 占比要在 {}% 到 {}% 之间。",
                Self::CPU.start(),
                Self::CPU.end()
            ));
        }
        Ok(())
    }

    /// The tokens past which the main agent is told its sub-agent nears the budget; none without
    /// a budget.
    #[must_use]
    pub fn warning_tokens(&self) -> Option<u64> {
        self.token_budget.map(|budget| budget / 100 * Self::WARNING_PERCENT)
    }
}

impl Default for AgentTeamLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The limits and permissions as the settings page reads them.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamSettingsView {
    pub limits: AgentTeamLimits,
    /// What 恢复默认 puts back.
    pub defaults: AgentTeamLimits,
    /// The smallest and the largest of each limit Core takes.
    pub minimum: AgentTeamLimits,
    pub maximum: AgentTeamLimits,
    pub permissions: crate::AgentTeamPermissions,
    /// What 恢复默认 puts back.
    pub permission_defaults: crate::AgentTeamPermissions,
    /// The settings revision a change names; 0 until the user saved any.
    pub revision: u64,
}

/// The user saves the limits, and the permissions when given, at the revision the page read.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamSettingsSetRequest {
    pub limits: AgentTeamLimits,
    /// None keeps the permissions saved before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<crate::AgentTeamPermissions>,
    pub expected_revision: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_within_every_range_and_each_range_is_held() {
        assert_eq!(AgentTeamLimits::DEFAULT.check(), Ok(()));
        // By default the tokens are not limited and a task has two hours.
        assert_eq!(AgentTeamLimits::DEFAULT.token_budget, None);
        assert_eq!(AgentTeamLimits::DEFAULT.warning_tokens(), None);
        assert_eq!(AgentTeamLimits::DEFAULT.minutes_per_task, 120);
        let budget = AgentTeamLimits { token_budget: Some(5_000_000), ..AgentTeamLimits::DEFAULT };
        assert_eq!(budget.warning_tokens(), Some(4_000_000));
        let refused = |limits: AgentTeamLimits| limits.check().map_err(|refusal| refusal.code);
        let base = AgentTeamLimits::DEFAULT;
        assert!(refused(AgentTeamLimits { max_running: 0, ..base }).is_err());
        assert!(refused(AgentTeamLimits { max_running: 9, ..base }).is_err());
        assert!(refused(AgentTeamLimits { token_budget: Some(99_999), ..base }).is_err());
        assert!(refused(AgentTeamLimits { token_budget: Some(100_000_001), ..base }).is_err());
        assert!(refused(AgentTeamLimits { minutes_per_task: 481, ..base }).is_err());
        assert!(refused(AgentTeamLimits { cpu_percent: 9, ..base }).is_err());
        assert!(refused(AgentTeamLimits { cpu_percent: 101, ..base }).is_err());
        let edges = AgentTeamLimits {
            max_running: 8,
            token_budget: Some(100_000),
            minutes_per_task: 5,
            cpu_percent: 100,
        };
        assert_eq!(edges.check(), Ok(()));
        assert_eq!(AgentTeamLimits::MINIMUM.check(), Ok(()));
        assert_eq!(AgentTeamLimits::MAXIMUM.check(), Ok(()));
    }
}
