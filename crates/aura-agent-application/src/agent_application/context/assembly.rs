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
    model_context::ModelHistory,
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
                    let mut history = model_context::history_for_model(
                        db,
                        run_id,
                        matches!(step.continuation, AgentContinuation::Initial { .. }),
                    )?;
                    if let AgentContinuation::QueryAnswered { user_event_id, .. } =
                        step.continuation
                    {
                        anyhow::ensure!(
                            matches!(history.items.last(),Some(LogicalConversationItem::User{source_event_id,..}) if *source_event_id==user_event_id),
                            "agent.query_answer_history"
                        );
                        history.items.pop();
                    }
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
                        "lineages":lineages,
                        "mode_episode":mode_episode,
                        "mode_disposition":disposition,
                        "current_user_root_event_id":turn.root_user_event_id,
                        "background_lineages_are_not_current_obligations":true,
                        "guard":guard,
                        "protocol_completion_reminder":protocol_completion,
                        "state_revision":revision,"state_hash":hash,"registry_snapshot_ref":registry.snapshot_ref,"permission_mode":permission.mode,"continuation":step.continuation
                    });
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
                        turn, run, bundle, prompt, settings, history, registry, loads,
                        tool_settings, package_tools, state,
                    ))
                },
            )
            .await?
        })
    }
}
