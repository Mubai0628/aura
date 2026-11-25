//! The application as the team's host: its store, its models, and the hidden conversations its
//! sub-agents work in.
use super::*;
use anyhow::Context as _;
use aura_agent_execution::command_execution::process_policy;
use aura_agent_team::{Delivery, ModelOffer, Parent, Progress, StopNote, Tally, TeamHost};
use aura_persistence_core::StoreResult;
use aura_persistence_v2::agent::{model_context, session};
use std::future::Future;

impl TeamHost for TeamApp {
    fn new_work_block(
        db: &aura_persistence_core::Connection,
        parent: SessionId,
    ) -> aura_persistence_core::StoreResult<Option<&'static str>> {
        super::admission::new_work_block(db, parent)
    }

    fn read<T, F>(
        &self,
        step: &'static str,
        work: F,
    ) -> impl Future<Output = anyhow::Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&aura_persistence_core::Connection) -> StoreResult<T> + Send + 'static,
    {
        let store = Arc::clone(&self.0.store);
        async move { Ok(store.read(StoreStep::operation(step), work).await?) }
    }

    fn write<T, F>(
        &self,
        step: &'static str,
        work: F,
    ) -> impl Future<Output = anyhow::Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&aura_persistence_core::Transaction<'_>) -> StoreResult<T> + Send + 'static,
    {
        let store = Arc::clone(&self.0.store);
        async move {
            Ok(store
                .write(StoreStep::operation(step), move |journal| {
                    journal.database.unit_of_work(work)
                })
                .await?)
        }
    }

    fn models(&self) -> anyhow::Result<Vec<ModelOffer>> {
        let config = self.0.p05.model_config.snapshot()?;
        let registry = &config.registry;
        let provider_of = |model: &aura_model_domain::ModelProfileConfig| {
            registry.endpoints.get(&model.endpoint_id).and_then(|endpoint| {
                registry.provider_order.iter().position(|id| *id == endpoint.provider_id)
            })
        };
        let mut models: Vec<_> = registry.models.values().filter(|model| model.enabled).collect();
        models.sort_by_key(|model| {
            (provider_of(model).unwrap_or(usize::MAX), model.display_name.clone())
        });
        Ok(models
            .into_iter()
            .map(|model| ModelOffer {
                id: model.id,
                name: model.display_name.clone(),
                for_subagents: model.available_for_subagents,
            })
            .collect())
    }

    fn parent(&self, session: SessionId) -> impl Future<Output = anyhow::Result<Parent>> + Send {
        let app = Arc::clone(&self.0);
        async move {
            let workspace_id = app.session_workspace(session).ok_or_else(|| {
                anyhow::anyhow!("agent.session_missing: 找不到这个会话，它可能已被删除。")
            })?;
            let model = app
                .execution
                .persistence
                .session_model_binding(session)?
                .map(|b| b.model_profile_id);
            let git = app
                .execution
                .workspace
                .resolver(workspace_id)
                .is_ok_and(|resolver| resolver.canonical_root().join(".git").exists());
            Ok(Parent { session_id: session, workspace_id, model, git })
        }
    }

    fn delegated_reads(
        &self,
        session: SessionId,
        turn: TurnTaskId,
        paths: Vec<String>,
    ) -> impl Future<Output = anyhow::Result<Option<aura_agent_team::DelegatedReads>>> + Send {
        let execution = Arc::clone(&self.0.execution);
        async move {
            if paths.is_empty() {
                return Ok(None);
            }
            tokio::task::spawn_blocking(move || {
                execution.prepare_delegated_reads(session, turn, &paths)
            })
            .await?
            .map(Some)
        }
    }

    fn open(
        &self,
        parent: &Parent,
        worktree: Option<WorkspaceId>,
        title: &str,
        model: ModelProfileId,
        mode: aura_security_contract::PermissionMode,
        command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<SessionId>> + Send {
        let app = Arc::clone(&self.0);
        let request = aura_workspace_domain::SessionCreateRequest {
            workspace_id: worktree.unwrap_or(parent.workspace_id),
            title: format!("子 Agent：{title}"),
        };
        // The name the user gave the model, for a refusal of it.
        let shown = self.0.p05.model_config.snapshot().ok().and_then(|config| {
            config.registry.models.get(&model).map(|profile| profile.display_name.clone())
        });
        async move {
            let persistence = Arc::clone(&app.execution.persistence);
            let correlation = CorrelationId::new();
            let child = tokio::task::spawn_blocking(move || -> anyhow::Result<SessionId> {
                let created = persistence.create_session(command, request, mode, correlation)?;
                persistence.mark_session_subagent(created.session_id)?;
                Ok(created.session_id)
            })
            .await??;
            app.p05.bind_model(child, model, correlation).map_err(|error| {
                let named =
                    shown.map_or_else(|| "选的模型".to_owned(), |name| format!("模型「{name}」"));
                anyhow::anyhow!(
                    "agents.model_unavailable: {named}现在不能给子 Agent 用：{}。子 Agent 没有开始干活；可以用 model 换一个允许子 Agent 用的模型再派。",
                    aura_agent_team::one_line(&error.message)
                )
            })?;
            Ok(child)
        }
    }

    fn discard(&self, child: SessionId) -> impl Future<Output = anyhow::Result<()>> + Send {
        let persistence = Arc::clone(&self.0.execution.persistence);
        async move {
            tokio::task::spawn_blocking(move || persistence.mark_session_deleting(child)).await??;
            Ok(())
        }
    }

    fn deliver(
        &self,
        child: SessionId,
        text: String,
        command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<Delivery>> + Send {
        let app = Arc::clone(&self.0);
        async move {
            let model =
                app.execution.persistence.session_model_binding(child)?.ok_or_else(|| {
                    anyhow::anyhow!("agents.model_unavailable: 子 Agent 没有绑定模型。")
                })?;
            app.require_temporary_model(model.model_profile_id)?;
            let active = app
                .store
                .read(StoreStep::operation("agents.deliver.turn"), move |db| {
                    super::admission::require_start(db, child)?;
                    Ok(session::load(db, child)?.active_turn)
                })
                .await?;
            let (lane, delivery) = match active {
                Some(turn_task_id) => {
                    (AgentInputLane::Addition { turn_task_id }, Delivery::AddedToTurn)
                }
                None => (AgentInputLane::NextTurn, Delivery::NewTurn),
            };
            app.submit_sent(command, AgentSubmitRequest::new(child, text, lane), None).await?;
            Ok(delivery)
        }
    }

    fn halt(
        &self,
        child: SessionId,
        note: Option<StopNote>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let app = Arc::clone(&self.0);
        async move {
            let run = app
                .store
                .read(StoreStep::operation("agents.halt.turn"), move |db| {
                    session::load(db, child)?
                        .active_turn
                        .map(|turn| turns::load(db, turn).map(|turn| turn.agent_run_id))
                        .transpose()
                })
                .await?;
            if let Some(agent_run_id) = run {
                let note = note
                    .map(|note| model_context::StopNote { clause: note.clause, next: note.next });
                app.cancel_noting(&AgentRunRequest { session_id: child, agent_run_id }, note)
                    .await?;
            }
            stop_processes(&app, child).await.context("停不掉它的命令启动的进程")?;
            process_policy::remove_temp_directory(child);
            app.execution.close_model_page(child).await;
            Ok(())
        }
    }

    fn progress(
        &self,
        conversation: SessionId,
    ) -> impl Future<Output = anyhow::Result<Progress>> + Send {
        let app = Arc::clone(&self.0);
        async move {
            let tokens = tokens(&app, conversation).await?;
            let tool = running_tool(&app, conversation);
            let answering = app.execution.team.is_answering(conversation);
            let persistence = Arc::clone(&app.execution.persistence);
            let mode = tokio::task::spawn_blocking(move || {
                persistence.session_permission_state(conversation).ok().map(|state| state.mode)
            })
            .await
            .ok()
            .flatten();
            let active = app
                .store
                .read(StoreStep::operation("agents.progress.turn"), move |db| {
                    let Some(turn) = session::load(db, conversation)?.active_turn else {
                        return Ok(None);
                    };
                    let loaded = turns::load(db, turn)?;
                    let retries = aura_persistence_v2::model::transient_retry::run_retries(
                        db,
                        loaded.agent_run_id,
                    )?;
                    Ok(Some((turn, loaded.agent_run_id, loaded.created_at, retries)))
                })
                .await?;
            let Some((turn, agent_run_id, began, (retries, failure))) = active else {
                return Ok(Progress { tokens, answering, mode, ..Progress::default() });
            };
            let status =
                app.status(&AgentRunRequest { session_id: conversation, agent_run_id }).await?;
            // A call waiting for the user has not started running: the list names the tool it
            // waits to run, as its approval does.
            let tool = match (tool, status.pending_approval_ids.first()) {
                (None, Some(&approval_id)) => app
                    .stored_approval(aura_execution_contract::ExecutionApprovalRequest {
                        session_id: conversation,
                        approval_id,
                    })
                    .await
                    .ok()
                    .map(|view| view.operation.content.action_kind),
                (tool, _) => tool,
            };
            let (approvals, reviewing) =
                app.split_reviews(conversation, status.pending_approval_ids).await?;
            let times = app.execution.team.answer_times(conversation);
            let progress = aura_agent_team::AgentProgressView {
                turn_seconds: u64::try_from(
                    (UtcTimestamp::now().value() - began.value()).whole_seconds(),
                )
                .unwrap_or(0),
                tool: tool.clone(),
                answer_seconds: times.map(|(answer, _)| answer.as_secs()),
                quiet_seconds: times.map(|(_, quiet)| quiet.as_secs()),
                retries,
                last_failure: failure.map(|failure| {
                    aura_persistence_v2::model::failure_text::cause_zh(&failure).to_owned()
                }),
            };
            Ok(Progress {
                working: true,
                turn: Some(turn),
                tool,
                approvals,
                reviewing,
                tokens,
                answering,
                mode,
                progress: Some(progress),
            })
        }
    }

    fn answer_approval(
        &self,
        child: SessionId,
        approval: ApprovalRequestId,
        answer: aura_agent_team::ApprovalAnswer,
        reason: &str,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let app = Arc::clone(&self.0);
        let reason = reason.to_owned();
        async move { app.answer_for_main_agent(child, approval, answer, &reason).await }
    }
}

/// Stops every process the conversation's commands left running; another conversation's go on.
pub(super) async fn stop_processes(
    app: &AgentApplication,
    session: SessionId,
) -> anyhow::Result<()> {
    for process in app.commands.process_list(session).await?.processes {
        app.commands
            .process_stop(aura_windows_user_contract::ManagedProcessStopRequestV1 {
                session_id: session,
                tool_call_id: process.tool_call_id,
            })
            .await?;
    }
    Ok(())
}

/// The tool the conversation runs now: its newest running call, or `agents.wait` while it waits
/// for its sub-agents before that call is admitted.
fn running_tool(app: &AgentApplication, conversation: SessionId) -> Option<String> {
    let calls = app.execution.active_calls.session(conversation).ok()?;
    calls
        .iter()
        .map(|active| &active.call.validated)
        .max_by_key(|validated| validated.call.tool_call_id)
        .map(|validated| validated.definition.canonical_name.to_string())
        .or_else(|| {
            app.execution.team.is_waiting(conversation).then(|| aura_agent_team::WAIT.to_owned())
        })
}

/// The tokens the conversation's answers used, counted on from where the last count ended.
async fn tokens(app: &Arc<AgentApplication>, conversation: SessionId) -> anyhow::Result<u64> {
    let from = app.execution.team.tally(conversation);
    let (after, added) = app
        .store
        .read(StoreStep::operation("agents.progress.tokens"), move |db| {
            aura_persistence_v2::agent::usage::tokens_after(db, conversation, from.after)
        })
        .await?;
    let to = Tally { after, tokens: from.tokens.saturating_add(added) };
    Ok(app.execution.team.settle_tally(conversation, from, to).tokens)
}
