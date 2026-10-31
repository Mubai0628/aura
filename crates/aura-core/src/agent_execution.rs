//! Agent execution is aura-agent-execution's. Core keeps its tests, which run a started Core
//! (`tests::fixture`); they read the module's names from here.
pub(crate) use aura_agent_execution::agent_execution::*;
#[cfg(test)]
use aura_domain_common::*;
#[cfg(test)]
use aura_execution_contract::*;
#[cfg(test)]
use aura_security_contract::*;
#[cfg(test)]
use aura_tool_contract::*;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
mod tests;
