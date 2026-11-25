//! The tool tables of the team: a sub-agent's is the main conversation's without the main agent's
//! own tools, or the list it was narrowed to, and its report tool. User conversations retain
//! management tools while new work is disabled; new work follows its current eligibility.
use super::*;
use aura_agent_team::{Member, policy};
use aura_tool_registry::RegistryCandidateV1;

/// The sub-agent working in `session`, if `session` is a sub-agent's conversation.
pub(in crate::agent_application) fn team_member(
    app: &Arc<AgentApplication>,
    session: SessionId,
) -> super::Boxed<'_, anyhow::Result<Option<Member>>> {
    Box::pin(async move {
        Ok(app
            .store
            .read(StoreStep::operation("agents.member"), move |db| member_of(db, session))
            .await?)
    })
}

/// Cuts the candidates down for the conversation's role. A sub-agent keeps every tool a sub-agent
/// may have, built-in, MCP or a package's, and its report tool; one the main agent narrowed keeps
/// only the tools it named, and has them all loaded: the tools returned. A conversation of the
/// user's never has the report tool. Its new-work tools follow eligibility; management remains
/// available, and agents.approve is offered only when the user enabled main-agent approvals.
pub(in crate::agent_application) fn restrict_candidates(
    member: Option<&Member>,
    availability: Availability,
    candidates: &mut Vec<RegistryCandidateV1>,
) -> Option<Vec<ToolId>> {
    if let Some(member) = member {
        let narrowed = member.details.grant.tools.as_deref();
        candidates.retain(|candidate| {
            aura_tool_registry::subagent_keeps(&candidate.definition, narrowed)
        });
        return narrowed
            .map(|_| candidates.iter().map(|candidate| candidate.definition.tool_id).collect());
    }
    candidates
        .retain(|candidate| candidate.definition.canonical_name.as_str() != policy::REPORT_TOOL);
    for candidate in candidates.iter_mut() {
        let name = candidate.definition.canonical_name.as_str();
        if policy::COORDINATOR_TOOLS.contains(&name)
            && (!availability.conversation
                || name == aura_agent_team::SPAWN && !availability.spawn
                || name == aura_agent_team::SEND && !availability.send
                || name == aura_agent_team::APPROVE && !availability.approves)
        {
            candidate.instance.runtime_health = aura_tool_domain::ToolRuntimeHealthV1::Unavailable;
        }
    }
    None
}

#[derive(Clone, Copy, Default)]
pub(in crate::agent_application) struct Availability {
    pub conversation: bool,
    pub spawn: bool,
    pub send: bool,
    pub approves: bool,
}

impl AgentApplication {
    pub(in crate::agent_application) fn team_availability(
        &self,
        db: &aura_persistence_core::Connection,
        session: SessionId,
        conversation: bool,
    ) -> aura_persistence_core::StoreResult<Availability> {
        if !conversation {
            return Ok(Availability::default());
        }
        let (_, permissions, _) = aura_agent_team::limits::settings(db)?;
        let enabled =
            super::admission::new_work_block(db, session)?.is_none() && self.team_offered();
        Ok(Availability {
            conversation,
            spawn: enabled,
            send: enabled,
            approves: permissions.main_agent_approves,
        })
    }

    /// Whether a conversation can start sub-agents: some enabled model is allowed for them.
    pub(in crate::agent_application) fn team_offered(&self) -> bool {
        self.p05.model_config.snapshot().is_ok_and(|config| {
            config
                .registry
                .models
                .values()
                .any(|model| model.enabled && model.available_for_subagents)
        })
    }
}
