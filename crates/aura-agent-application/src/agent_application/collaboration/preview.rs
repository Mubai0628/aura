use super::*;
use aura_agent_collaboration::{
    AgentProfileGeneration, AgentProfileGenerationState, AgentProfilePreviewRequest,
};
use aura_persistence_core::PersistenceCoreError as Error;

impl AgentApplication {
    pub(super) async fn preview_profile(
        self: &Arc<Self>,
        request: AgentProfilePreviewRequest,
    ) -> anyhow::Result<AgentProfile> {
        self.ensure_session(request.session_id).await?;
        let session = request.session_id;
        let key = format!("preview:{}", ModelStepId::new());
        let work = self.store.write(StoreStep::operation("agents.profile.preview.reserve"), move |journal| {
            journal.database.unit_of_work(|tx| {
                let metadata = aura_persistence_v2::workspace::session(tx, session)?.ok_or(Error::Invalid("session"))?;
                let previous = profile::load(tx, session, metadata.workspace_id, &metadata.title)?;
                if previous.revision != request.expected_revision { return Err(Error::Conflict); }
                let run: String = tx.query_row(
                    "SELECT r.id FROM agent_runs r JOIN turn_tasks t ON t.id=r.turn_task_id WHERE t.session_id=?1 AND t.state='completed' ORDER BY r.rowid DESC LIMIT 1",
                    [session.to_string()], |r| r.get(0),
                )?;
                let run: AgentRunId = run.parse().map_err(|_| Error::Invalid("source run"))?;
                let turn = turns::load(tx, turns::load_run(tx, run)?.turn_task_id)?;
                if !matches!(turn.state, TurnTaskState::Completed { .. }) { return Err(Error::Conflict); }
                let record = AgentProfileGeneration { source_run_id: run, source_turn_id: turn.turn_task_id,
                    model_profile_id: None, state: AgentProfileGenerationState::Running,
                    input_tokens: 0, output_tokens: 0, usage_complete: false, message: None, preview: None };
                if !profile::reserve_generation_as(tx, &key, session, previous.revision, &record, UtcTimestamp::now())? {
                    return Err(Error::Invalid("profile generation already running"));
                }
                Ok(ProfileWork { run, previous, record, key, preview: true })
            })
        }).await?;
        let app = self.clone();
        let key = work.key.clone();
        tokio::spawn(async move {
            if !matches!(
                crate::agent_supervision::CatchUnwind::new(app.run_profile_generation(work)).await,
                Ok(Ok(()))
            ) {
                let settlement_recorded = app.fail_profile_work(key).await.is_ok();
                append_core_log(
                    "agents.profile_preview_failed",
                    serde_json::json!({"session":session,"settlement_recorded":settlement_recorded}),
                );
                if !settlement_recorded {
                    return;
                }
            }
            if let Ok(Some(run)) = app.store.read(StoreStep::operation("agents.profile.latest"), move |db| {
                use aura_persistence_core::OptionalExtension;
                let value: Option<String> = db.query_row("SELECT r.id FROM agent_runs r JOIN turn_tasks t ON t.id=r.turn_task_id WHERE t.session_id=?1 AND t.state='completed' ORDER BY r.rowid DESC LIMIT 1", [session.to_string()], |row| row.get(0)).optional()?;
                value.map(|id| id.parse::<AgentRunId>().map_err(|_| Error::Invalid("source run"))).transpose()
            }).await {
                let _ = app.schedule_profile(run).await;
            }
        });
        self.agent_profile(session).await
    }
}
