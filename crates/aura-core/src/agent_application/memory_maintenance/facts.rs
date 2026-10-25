use super::*;
use aura_memory::MemoryWorkEvidence;
#[derive(Clone)]
pub(in crate::agent_application) struct Pending {
    pub run: AgentRunId,
    pub turn: TurnTask,
    pub workspace: MemoryWorkspace,
    pub registry: aura_tool_contract::ToolRegistrySnapshotRef,
    pub bundle: PromptBundleSnapshotId,
    pub input: String,
    pub record: MemoryMaintenanceRun,
}
impl AgentApplication {
    pub(in crate::agent_application) async fn memory_facts(
        &self,
        run: AgentRunId,
    ) -> anyhow::Result<Option<Pending>> {
        let loaded=self.store.read(StoreStep::run(run,"memory.sources"),move |db|{
            let source=turns::load_run(db,run)?;let turn=turns::load(db,source.turn_task_id)?;
            let TurnTaskState::Completed {ref final_report}=turn.state else {return Ok(None)};
            let mut statement=db.prepare("SELECT c.id FROM tool_calls c JOIN tool_batches b ON b.id=c.batch_id WHERE b.session_id=?1 AND json_extract(b.payload,'$.batch.invocation_identity.agent_run_id')=?2 ORDER BY c.rowid DESC LIMIT 100")?;
            let ids=statement.query_map([turn.session_id.to_string(),run.to_string()],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?.into_iter().map(|s|s.parse().map_err(|_|aura_persistence_core::PersistenceCoreError::Conflict)).collect::<aura_persistence_core::StoreResult<Vec<ToolCallId>>>()?;
            if ids.is_empty(){return Ok(None);}
            let page=aura_persistence_v2::tool::activity::page(db,&aura_tool_projection::AgentToolActivityRequest {session_id:turn.session_id,before_call_id:None,limit:100,call_ids:Some(ids)})?;
            let effects=page.calls.iter().filter(|c|matches!(c.summary.side_effect_state,Some(aura_execution_contract::SideEffectStateV1::FullyApplied|aura_execution_contract::SideEffectStateV1::PartiallyApplied|aura_execution_contract::SideEffectStateV1::Unassessed)) && matches!(c.summary.outcome,Some(aura_tool_projection::ToolResultOutcomeSummaryView::Succeeded))).count() as u32;
            let executed=page.calls.iter().filter(|c|c.summary.started_at.is_some()).count() as u32;
            let evidence=MemoryWorkEvidence {normally_completed:true,side_effect_calls:effects,tool_calls:executed,answer_characters:final_report.chars().count()};
            if evidence.trigger().is_none(){return Ok(None);}
            let batch=aura_persistence_v2::tool::execution::batch(db,page.calls[0].summary.tool_batch_id)?;
            let config=store::configuration(db,run)?;
            let input=serde_json::json!({"task":bounded(&config.request.text,8000),"completed_answer":bounded(final_report,12000),"tools":page.calls.iter().take(20).map(|c|serde_json::json!({"title":bounded(&c.display.title,200),"summary":bounded(&c.display.summary,400),"side_effect":c.summary.side_effect_state})).collect::<Vec<_>>()});
            Ok(Some((turn,source.prompt_bundle_snapshot_id,batch.batch.registry_snapshot_ref,input)))
        }).await?;
        let Some((turn, bundle, registry, mut input)) = loaded else { return Ok(None) };
        let resolver = self.execution.workspace.resolver(turn.workspace_id)?;
        resolver.verify_root_identity()?;
        let workspace = MemoryWorkspace {
            workspace_id: turn.workspace_id,
            root: resolver.canonical_root().to_path_buf(),
        };
        let memory = self.execution.journal.memory().clone();
        let other = workspace.clone();
        let runtime = self.execution.runtime;
        let id = ContentHash::blake3(run.to_string().as_bytes()).hex;
        let id_check = id.clone();
        let recall=tokio::task::spawn_blocking(move ||->anyhow::Result<Option<serde_json::Value>> {
            let project=memory.vault(MemoryScope::Project,&other)?;
            let _=project.maintenance_runs(runtime)?;
            if project.maintenance_run(&id_check)?.is_some(){return Ok(None);}
            let mut recall=vec![];
            for scope in [MemoryScope::Project,MemoryScope::Global] {
                let vault=memory.vault(scope,&other)?;
                for hit in vault.recent(Some(10))?.hits {
                    let note=vault.read_id(hit.metadata.id)?;
                    recall.push(serde_json::json!({"scope":scope,"id":hit.metadata.id,"hash":note.hash,"body":bounded(&note.note.body,1000)}));
                }
            }
            Ok(Some(serde_json::json!(recall)))
        }).await??;
        let Some(recall) = recall else { return Ok(None) };
        input["existing_memories"] = recall;
        let input = serde_json::to_string(&input)?;
        let record = MemoryMaintenanceRun {
            run_id: id,
            runtime_instance_id: runtime,
            session_id: turn.session_id,
            turn_id: Some(turn.turn_task_id.to_string()),
            state: MemoryMaintenanceState::Queued,
            reason: None,
            model_profile_id: None,
            input_tokens: 0,
            output_tokens: 0,
            usage_complete: true,
            started_at: UtcTimestamp::now(),
            finished_at: None,
        };
        Ok(Some(Pending { run, turn, workspace, registry, bundle, input, record }))
    }
}
fn bounded(text: &str, characters: usize) -> String {
    text.chars().take(characters).collect()
}
