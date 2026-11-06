//! Skills in a step's context. The pinned Skills and the names of the other usable ones go ahead
//! of the workspace's AGENTS.md, decided at the run's first step and kept for its others. A Skill
//! the model loads is a tool result in the conversation, and one the user chose is part of their
//! message, so neither is here. Nothing here is a user request or a permission.
use super::*;
use aura_extension_service::skills::instructions;
use aura_persistence_v2::agent::skills::TurnSkills;

fn skill_fragment(state_kind: &str, revision: Revision, content: String) -> ContextFragment {
    ContextFragment {
        source_ref: ContextSourceRef::RuntimeState {
            state_kind: state_kind.into(),
            state_revision: revision,
        },
        source_kind: ModelEgressSourceKind::Skill,
        data_class: NetworkDataClass::UserFileContent,
        authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
        content,
    }
}

impl AgentApplication {
    /// The pinned Skills whole, then the list of the other names, for the instructions ahead of
    /// AGENTS.md. The run's first step decides both and records the decision, so a Skill added,
    /// pinned or switched on while the turn runs changes nothing until the next turn; a pinned
    /// Skill is read again each step and left out once it is no longer usable. A Skill folder
    /// that cannot be read leaves the Skills out rather than the step.
    pub(super) async fn turn_skill_fragments(
        &self,
        run: AgentRunId,
        workspace: WorkspaceId,
        revision: Revision,
    ) -> anyhow::Result<Vec<ContextFragment>> {
        let Some(runtime) = self.execution.skills.get().cloned() else { return Ok(Vec::new()) };
        let recorded = self
            .store
            .read(StoreStep::run(run, "agent.context.turn_skills"), move |db| {
                aura_persistence_v2::agent::skills::turn_skills(db, run)
            })
            .await?;
        let decided = match recorded {
            Some(decided) => decided,
            None => {
                let reading = Arc::clone(&runtime);
                let decided = tokio::task::spawn_blocking(move || {
                    instructions::for_turn(&reading, Some(workspace))
                })
                .await
                .map_err(anyhow::Error::from)
                .and_then(|decided| decided)
                .map(|decided| TurnSkills { pinned: decided.pinned, list: decided.list })
                .unwrap_or_default();
                let journal = Arc::clone(&self.execution.journal);
                let record = decided.clone();
                let at = UtcTimestamp::now();
                self.store
                    .run_critical(
                        StoreStep::run(run, "agent.context.turn_skills_record"),
                        StoreAccess::Write,
                        move || {
                            Ok(journal.lock()?.database.unit_of_work(|tx| {
                                aura_persistence_v2::agent::skills::set_turn_skills(
                                    tx, run, &record, at,
                                )
                            })?)
                        },
                    )
                    .await?;
                decided
            }
        };
        let pinned = if decided.pinned.is_empty() {
            Vec::new()
        } else {
            let ids = decided.pinned.clone();
            tokio::task::spawn_blocking(move || {
                instructions::pinned_texts(&runtime, Some(workspace), &ids)
            })
            .await
            .map_err(anyhow::Error::from)
            .and_then(|texts| texts)
            .unwrap_or_default()
        };
        Ok(pinned
            .into_iter()
            .map(|text| skill_fragment("skill.pinned.v1", revision, text))
            .chain(decided.list.map(|list| skill_fragment("skill.names.v1", revision, list)))
            .collect())
    }
}
