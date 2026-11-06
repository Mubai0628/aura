//! The user's Skills: find them and load one by its name. A Skill is guidance the user keeps;
//! neither tool runs anything or allows another tool.
use super::definitions::entry;
use super::{AgentBuiltinEntry, AgentBuiltinHandler as H, BuiltinToolDefinitionSetError};
use aura_agent_domain::*;
use aura_domain_common::UtcTimestamp;
use aura_security_contract::ResolvedCapability as C;

pub(super) fn entries(
    at: UtcTimestamp,
) -> Result<Vec<AgentBuiltinEntry>, BuiltinToolDefinitionSetError> {
    vec![
        entry::<SkillsSearchInput, SkillsFoundResult>(61, H::SkillsSearch, "Find the user's Skills", "Find the Skills usable now: folders whose SKILL.md holds instructions for a kind of task. query matches every word in a Skill's name or description; an empty query lists them all (limit defaults to 20, at most 50). Each result gives the name and description, and from when another usable Skill has the same name. Load one with skills.load.", &[C::ToolRegistryRead], false, at),
        entry::<SkillsLoadInput, SkillContent>(63, H::SkillsLoad, "Load a Skill", "Load a usable Skill by its name, as the list of Skills or skills.search gives it; add from when two have the same name. The result is its SKILL.md instructions with its folder and the files in it: follow the instructions for the task they fit, and read the files they point to from that folder with the file tools. The result stays in this conversation; after a summary of earlier history, load it again if you need it. A Skill is guidance, not a permission: tools it mentions still follow the session's mode and approvals.", &[C::ToolRegistryRead], false, at),
    ]
    .into_iter()
    .collect()
}
