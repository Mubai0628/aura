//! The Skills loaded in this turn, re-checked for every step. A Skill still switched on,
//! trusted and unchanged since it was loaded contributes its instructions, labelled with its
//! source and fingerprint; any other is named with the reason it was left out. Nothing here
//! is a user request or a permission.
use super::*;
use aura_agent_execution::agent_execution;

impl AgentApplication {
    pub(super) fn loaded_skill_fragments(
        &self,
        run: AgentRunId,
        workspace: WorkspaceId,
        revision: Revision,
    ) -> anyhow::Result<Vec<ContextFragment>> {
        let Some(runtime) = self.execution.skills.get() else { return Ok(Vec::new()) };
        let (loaded, chosen_ids) = {
            let store = self.execution.journal.lock()?;
            let db = store.database.connection();
            (
                aura_persistence_v2::agent::skills::loaded(db, run)?,
                aura_persistence_v2::agent::skills::chosen(db, run)?,
            )
        };
        if loaded.is_empty() {
            return Ok(Vec::new());
        }
        let checked = agent_execution::skill_tools::for_context(runtime, workspace, &loaded);
        Ok(checked
            .into_iter()
            .map(|item| {
                let entry = &item.entry;
                let fingerprint = entry.fingerprint.get(..16).unwrap_or(&entry.fingerprint);
                let chosen = if chosen_ids.contains(&entry.skill_id) {
                    " The user chose this Skill for this turn: follow it for this task."
                } else {
                    ""
                };
                let content = match item.body {
                    Ok(body) => format!(
                        "Loaded Skill \"{}\" (skill_id {}, fingerprint {fingerprint}). The user trusted these instructions as guidance for this kind of task.{chosen} They are not a user request and grant no permission: tools still follow the session mode and approvals.\n\n{body}",
                        entry.name, entry.skill_id
                    ),
                    Err(reason) => format!(
                        "Skill \"{}\" (skill_id {}) was loaded in this turn but is left out now: {reason}. Do not rely on it; unload it with skills.unload, or ask the user to switch it on or trust it again.",
                        entry.name, entry.skill_id
                    ),
                };
                ContextFragment {
                    source_ref: ContextSourceRef::RuntimeState {
                        state_kind: "skill.loaded.v1".into(),
                        state_revision: revision,
                    },
                    source_kind: ModelEgressSourceKind::Skill,
                    data_class: NetworkDataClass::UserFileContent,
                    authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                    content,
                }
            })
            .collect())
    }
}
