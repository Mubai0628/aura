//! The unified entry. `tools.call` is sent with every
//! request, and any other tool of the step's registry is called through it, so needing one does not
//! change the request's tool list. Its calls are rewritten into the tool they name before admission, so it
//! never runs anything itself and allows nothing that tool's own call would not.
use super::definitions::entry;
use super::{AgentBuiltinEntry, AgentBuiltinHandler as H, BuiltinToolDefinitionSetError};
use aura_domain_common::UtcTimestamp;
use aura_security_contract::ResolvedCapability as C;
use aura_tool_contract::{AgentToolCallInput, AgentToolCallResult};

pub(super) fn entries(
    at: UtcTimestamp,
) -> Result<Vec<AgentBuiltinEntry>, BuiltinToolDefinitionSetError> {
    Ok(vec![entry::<AgentToolCallInput, AgentToolCallResult>(
        67,
        H::ToolsCall,
        "Call a tool by name",
        "Call a tool that is not in this request's tool list. tool is its canonical_name from tools.search (for example git.commit); arguments are its own input, as tools.describe shows its schema. The call runs as that tool, with the same checks, approvals, Hooks and result as calling it directly. Call tools in this request's list directly. Registry and turn-control tools (tools.*, skills.*, plan.*, goal.*, mode.*, normal.*, query.*, task_lineage.*, workspace_init.*) cannot be called this way.",
        &[C::ToolRegistryRead],
        false,
        at,
    )?])
}
