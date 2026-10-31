//! A new model boundary reads live settings; earlier snapshots remain immutable.
//!
//! The prompt refresh is one step of a run's context preparation, so its reads and its
//! transaction run on the bounded Store owner like the rest of the run path.
use super::*;
use aura_core_broker::agent_store::StoreStep;
use aura_persistence_v2::{agent::model_context, context};

impl AgentApplication {
    pub(super) async fn refresh_prompt(&self, id: AgentRunId) -> anyhow::Result<()> {
        let settings = self.settings.prompt_snapshot()?;
        let (run, turn) = self
            .store
            .read(StoreStep::run(id, "agent.refresh_prompt.load"), move |db| {
                let run = turns::load_run(db, id)?;
                let turn = turns::load(db, run.turn_task_id)?;
                Ok((run, turn))
            })
            .await?;
        let previous = self
            .store
            .read(StoreStep::run(id, "agent.refresh_prompt.bundle"), move |db| {
                model_context::bundle(db, run.prompt_bundle_snapshot_id, turn.session_id)
            })
            .await?;
        if previous.settings_revision.value() == settings.revision {
            return Ok(());
        }
        let at = UtcTimestamp::now();
        let bundle = aura_prompt_runtime::freeze_prompt_bundle(
            &aura_prompt_runtime::ValidatedPromptBundle::embedded()?,
            &settings,
            turn.turn_task_id,
            env!("CARGO_PKG_VERSION"),
            at,
        )?;
        let session_id = turn.session_id;
        self.store
            .write(StoreStep::run(id, "agent.refresh_prompt.save"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    for value in &bundle.artifacts {
                        aura_persistence_core::artifact::put(
                            tx,
                            value.artifact_id,
                            &bundle.snapshot.id.to_string(),
                            &value.media_type,
                            &value.bytes,
                            at,
                        )?;
                    }
                    context::insert_bundle(tx, session_id, &bundle.snapshot)?;
                    let mut next = run.clone();
                    next.prompt_bundle_snapshot_id = bundle.snapshot.id;
                    next.revision = Revision::new(run.revision.value() + 1);
                    next.updated_at = at;
                    turns::save_run(tx, &run, &next)
                })
            })
            .await?;
        Ok(())
    }
}
