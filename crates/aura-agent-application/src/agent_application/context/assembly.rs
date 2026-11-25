use super::*;
use aura_agent_execution::agent_execution;
use aura_tool_contract::{ToolRegistrySnapshotV1, ToolSettingsSnapshotV1};
use aura_tool_domain::ToolEphemeralLoadRecordV1;

const CHILD_SESSION: &str = "Computer control works in the 小窗, a separate Windows session the user watches beside Aura. Shell and terminal commands still run on the user's own desktop: a program or page they open appears there, not in the 小窗. Open programs with computer.open and work in them with the computer tools, even where a command would be quicker; to show a page, open the browser with computer.open and enter the address in it. Aura refuses a script that would open a page, a browser or a folder window.";

pub(super) type FrozenFacts = (
    TurnTask,
    AgentRun,
    PromptBundleSnapshot,
    ValidatedPromptBundle,
    aura_config::PromptSettingsSnapshot,
    Option<(EventId, String)>,
    ToolRegistrySnapshotV1,
    Vec<ToolEphemeralLoadRecordV1>,
    ToolSettingsSnapshotV1,
    agent_execution::package_tools::SnapshotPackageTools,
    serde_json::Value,
);

impl AgentApplication {
    pub(super) async fn assemble_frozen_facts(
        self: &Arc<Self>,
        run_id: AgentRunId,
        permission: &PermissionSnapshot,
        step: &ModelStep,
        at: UtcTimestamp,
        computer: bool,
    ) -> anyhow::Result<FrozenFacts> {
        Ok({
            // Boundary: this is ONE contiguous synchronous critical section - it holds the
            // journal guard across ten reads, the checkpoint pair and the registry build - and it is
            // moved whole. Splitting it would change what "one consistent context picture" means, so
            // it is not split; it is only moved off the reactive task onto the one bounded owner.
            let app = Arc::clone(self);
            let permission = (*permission).clone();
            let step = (*step).clone();
            self.store
            .run_critical(
                StoreStep::run(run_id, "agent.context.assemble"),
                StoreAccess::Read,
                move || -> anyhow::Result<_> {
                    let store = app.execution.journal.lock()?;
                    let db = store.database.connection();
                    let run = turns::load_run(db, run_id)?;
                    let turn = turns::load(db, run.turn_task_id)?;
                    let bundle =
                        model_context::bundle(db, run.prompt_bundle_snapshot_id, turn.session_id)?;
                    let (prompt, settings) = frozen_prompt(&bundle, |id| {
                        Ok(artifact::read(db, id, &bundle.id.to_string())?)
                    })?;
                    let latest_input = source_input(db, &turn, &step.continuation)?;
                    let (registry, loads, tool_settings, package_tools) =
                        app.prepare_registry(db, &turn, &permission, at, computer)?;
                    let (revision, hash) = checkpoint::current_state(db, turn.turn_task_id)?;
                    let session_state = session::load(db, turn.session_id)?;
                    let lineages = session_state
                        .open_lineages
                        .iter()
                        .map(|id| lineage::load(db, *id))
                        .collect::<Result<Vec<_>, _>>()?;
                    let mode_episode = aura_persistence_v2::agent::mode::view(db, turn.session_id)?;
                    let guard = aura_persistence_v2::agent::guard::load(db, turn.turn_task_id)?;
                    let delivery = aura_persistence_v2::agent::peer_input::delivery(db, &turn)?;
                    let peer = delivery.is_some();
                    let protocol_completion = match &step.continuation {
                        AgentContinuation::CompletionReminder { previous_model_step_id } => {
                            let reminder = protocol_completion_reminder(
                                turn.mode,
                                mode_episode.disposition_required,
                            );
                            let previous = aura_persistence_v2::agent::history::output(
                                db,
                                *previous_model_step_id,
                            )?;
                            Some(if cut_off_while_reasoning(&previous) {
                                output_limit_reminder(reminder)
                            } else {
                                reminder
                            })
                        }
                        _ => None,
                    };
                    let disposition =
                        mode_episode.disposition_required.then_some(serde_json::json!({
                            "required": true,
                            "instruction": "This Plan or Goal was interrupted or paused. Before any other work, call mode.disposition. continue keeps that mode and, for Goal, ends the pause without resetting usage. skip marks the episode skipped and this turn stays Normal. If intent is unclear, call query.user_question. Do not infer the decision from keywords or from this reminder."
                        }));
                    let mut state = serde_json::json!({
                        "turn":turn,"run":run,"session":session_state,"todo":work::todo(db,turn.turn_task_id)?,
                        "query":work::query(db,turn.turn_task_id)?,"goal":if turn.mode==AgentTaskMode::Goal {Some(work::goal(db,turn.turn_task_id)?)}else{None},
                        "related_source":aura_persistence_v2::agent::related::context(db,turn.turn_task_id)?,
                        "lineages":lineages.into_iter().filter(|l| if peer { l.lineage_id == turn.lineage_id } else { !l.sources.is_empty() }).collect::<Vec<_>>(),
                        "mode_episode":mode_episode,
                        "mode_disposition":disposition,
                        "current_user_root_event_id":(!peer).then_some(turn.root_user_event_id),
                        "background_lineages_are_not_current_obligations":true,
                        "guard":guard,
                        "protocol_completion_reminder":protocol_completion,
                        "state_revision":revision,"state_hash":hash,"registry_snapshot_ref":registry.snapshot_ref,"permission_mode":permission.mode,"continuation":step.continuation
                    });
                    if let Some(peers) = app.peer_prompt_state(db, &turn, delivery.as_ref())? {
                        state["peer_collaboration"] = peers;
                    }
                    // The tools' own descriptions send a model to the command line first; in the
                    // 小窗 that opens things on the user's desktop instead.
                    if computer
                        && aura_persistence_v2::agent::application::computer_mode(db, run_id)?
                            .is_some_and(|mode| !mode.is_foreground())
                    {
                        state["computer_control"] = serde_json::json!({
                            "desktop": "child_session",
                            "instruction": CHILD_SESSION,
                        });
                    }
                    Ok((
                        turn, run, bundle, prompt, settings, latest_input, registry, loads,
                        tool_settings, package_tools, state,
                    ))
                },
            )
            .await?
        })
    }

    pub(super) async fn authorized_history(
        &self,
        turn: &TurnTask,
        step: &ModelStep,
        budget: u64,
        access: super::compaction::dependencies::Access,
    ) -> anyhow::Result<(model_context::ModelHistory, super::compaction::AuthorizedCheckpoint)>
    {
        let (run, session, step_id) = (turn.agent_run_id, turn.session_id, step.model_step_id);
        let continuation = step.continuation.clone();
        Ok(self.store
            .read(StoreStep::run(run, "agent.context.history"), move |db| {
                let started = std::time::Instant::now();
                let checkpoint = super::compaction::authorized_checkpoint(db, session, run, &access)?;
                let mut history = model_context::history_for_context(
                    db,
                    run,
                    matches!(continuation, AgentContinuation::Initial { .. }),
                    budget,
                        checkpoint.record.as_deref(),
                )?;
                aura_core_base::append_core_log("context.history_read", serde_json::json!({
                    "model_step_id":step_id, "elapsed_ms":started.elapsed().as_millis(),
                    "covered_events":history.covered_events, "deferred_groups":history.deferred_groups,
                    "selected_history_bytes":history.selected_history_bytes, "loaded_items":history.items.len(),
                    "compaction_pressure":history.compaction_pressure,
                    "missing_adopted_groups":history.missing_adopted.len(),
                    "checkpoint":checkpoint.record.as_ref().map(|record| record.id),
                }));
                if let AgentContinuation::QueryAnswered { user_event_id, .. } = continuation {
                    if !matches!(history.items.last(),Some(LogicalConversationItem::User{source_event_id,..}) if *source_event_id==user_event_id) {
                        return Err(aura_persistence_core::PersistenceCoreError::Invalid("agent.query_answer_history"));
                    }
                    history.items.pop();
                }
                Ok((history, checkpoint))
            })
            .await?)
    }
}

/// Find the latest accepted user input for source grants without reading optional history.
fn source_input(
    db: &aura_persistence_core::Connection,
    turn: &TurnTask,
    continuation: &AgentContinuation,
) -> anyhow::Result<Option<(EventId, String)>> {
    let message = |id| -> anyhow::Result<_> {
        let user = session::user_message(db, turn.session_id, id)?;
        Ok(Some((user.event_id, user.text)))
    };
    if let AgentContinuation::QueryAnswered { user_event_id, .. } = continuation {
        return message(*user_event_id);
    }
    let mut additions = aura_persistence_v2::agent::inbox::consumed(db, turn.turn_task_id)?
        .into_iter()
        .flat_map(|group| group.user_event_ids)
        .filter(|id| *id != turn.root_user_event_id)
        .collect::<Vec<_>>();
    if matches!(continuation, AgentContinuation::Initial { .. }) {
        if let Some(peer) = super::super::collaboration::peer_working_set(db, turn)? {
            return Ok(Some(peer));
        }
        return message(additions.last().copied().unwrap_or(turn.root_user_event_id));
    }
    additions.extend(aura_persistence_v2::agent::query::events(db, turn.turn_task_id)?);
    if !additions.is_empty() {
        let latest = db.query_row(
            "SELECT (SELECT e.event_id FROM events e
             JOIN json_each(?1) a ON a.value=e.event_id
             WHERE e.kind='agent.user_message' ORDER BY e.sequence DESC LIMIT 1)",
            [serde_json::to_string(&additions)?],
            |row| row.get::<_, Option<String>>(0),
        )?;
        return match latest {
            Some(id) => message(id.parse()?),
            None => Ok(None),
        };
    }
    if let Some(peer) = super::super::collaboration::peer_working_set(db, turn)? {
        return Ok(Some(peer));
    }
    message(turn.root_user_event_id)
}
