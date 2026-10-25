//! Desktop input lanes and queue scheduling. The queue never chooses a model,
//! a permission mode, or a special task mode.
use super::*;
use crate::agent_store::StoreStep;
use aura_persistence_v2::agent::{inbox, session};

impl AgentApplication {
    /// Mirrors one primary-registry Session (and its Workspace) into the V2
    /// execution store.
    ///
    /// Both the queue path and the explicit mode-start path must use this, and
    /// the mirror follows the authority: an unnamed draft Session stays
    /// unnamed, and the title written by the first accepted message converges
    /// on the next agent call instead of drifting.
    pub(super) fn mirror_session(
        tx: &aura_persistence_core::Transaction<'_>,
        workspace: &aura_workspace_domain::WorkspaceSummary,
        metadata: &aura_workspace_domain::SessionSummary,
    ) -> aura_persistence_core::StoreResult<()> {
        if aura_persistence_v2::workspace::get(tx, workspace.workspace_id)?.is_none() {
            aura_persistence_v2::workspace::insert(tx, workspace)?;
        }
        let at = UtcTimestamp::now();
        match aura_persistence_v2::workspace::session(tx, metadata.session_id)? {
            Some(current) if current.title != metadata.title => {
                aura_persistence_v2::workspace::rename_session(
                    tx,
                    metadata.session_id,
                    &metadata.title,
                    at,
                )
            }
            Some(_) => Ok(()),
            None => aura_persistence_v2::workspace::insert_session(
                tx,
                &aura_persistence_v2::workspace::SessionMetadata {
                    id: metadata.session_id,
                    workspace_id: metadata.workspace_id,
                    title: metadata.title.clone(),
                    created_at: at,
                    updated_at: at,
                },
            ),
        }
    }

    pub(super) async fn ensure_session(self: &Arc<Self>, id: SessionId) -> anyhow::Result<()> {
        let projection = self.execution.persistence.workspace_session_projection()?;
        let metadata = projection
            .sessions
            .iter()
            .find(|s| s.session_id == id)
            .ok_or_else(|| anyhow::anyhow!("agent.session_missing"))?
            .clone();
        let workspace = projection
            .workspaces
            .iter()
            .find(|w| w.workspace_id == metadata.workspace_id)
            .ok_or_else(|| anyhow::anyhow!("agent.workspace_missing"))?
            .clone();
        // Every read
        // of a conversation wrote this mirror again, a transaction queued behind every other
        // write even when nothing had changed. It is written only when it differs.
        let (workspace_id, session_id, title) =
            (workspace.workspace_id, metadata.session_id, metadata.title.clone());
        let current = self
            .store
            .read(StoreStep::operation("agent.session.mirror_check"), move |db| {
                Ok(aura_persistence_v2::workspace::get(db, workspace_id)?.is_some()
                    && aura_persistence_v2::workspace::session(db, session_id)?
                        .is_some_and(|session| session.title == title))
            })
            .await?;
        if current {
            return Ok(());
        }
        self.store
            .write(StoreStep::operation("agent.session.mirror"), move |journal| {
                journal
                    .database
                    .unit_of_work(|tx| AgentApplication::mirror_session(tx, &workspace, &metadata))
            })
            .await?;
        Ok(())
    }

    /// The conversations with a turn under way, for the sidebar's running state.
    pub async fn sessions_activity(self: &Arc<Self>) -> anyhow::Result<AgentSessionsActivity> {
        Ok(self
            .store
            .read(StoreStep::operation("agent.sessions.activity"), move |db| {
                aura_persistence_v2::agent::under_way::sessions(db)
            })
            .await?)
    }

    pub async fn inbox(
        self: &Arc<Self>,
        request: AgentSessionRequest,
    ) -> anyhow::Result<AgentInboxProjection> {
        self.ensure_session(request.session_id).await?;
        Ok(self
            .store
            .read(StoreStep::operation("agent.inbox.load"), move |db| {
                inbox::load(db, request.session_id)
            })
            .await?)
    }

    pub async fn submit(
        self: &Arc<Self>,
        command: CommandInvocationId,
        request: AgentSubmitRequest,
    ) -> anyhow::Result<AgentInboxProjection> {
        self.ensure_session(request.session_id).await?;
        let repeated = request.clone();
        if self
            .store
            .read(StoreStep::operation("agent.inbox.submitted"), move |db| {
                inbox::submitted(db, command, &repeated)
            })
            .await?
        {
            return self.inbox(AgentSessionRequest { session_id: request.session_id }).await;
        }
        let lane = match request.destination {
            AgentInputLane::NextTurn => "next_turn",
            AgentInputLane::Addition { .. } => "addition",
        };
        let workspace = self.session_workspace(request.session_id);
        let skills = self.selected_skills(workspace, &request.skill_ids).await?;
        anyhow::ensure!(
            skills.is_empty() || matches!(request.destination, AgentInputLane::NextTurn),
            "agent.skills_need_new_turn: 选择的 Skill 只在开始新的一轮时使用"
        );
        // The pictures and files sent with the message, checked for the session's model.
        let model = self
            .execution
            .persistence
            .session_model_binding(request.session_id)?
            .map(|binding| binding.model_profile_id);
        let prepared = self
            .prepare_attachments(
                command,
                request.session_id,
                workspace,
                model,
                &request.attachments,
            )
            .await?;
        let rewrite =
            self.accept_message(request.session_id, workspace, &request.text, lane).await?;
        let stored = request.clone();
        let repeated = request.clone();
        self.accept_attachments(
            "agent.inbox.submit",
            request.session_id,
            prepared.grants,
            move |db| {
                if inbox::submitted(db, command, &repeated)? {
                    Ok(Some(inbox::load(db, repeated.session_id)?))
                } else {
                    Ok(None)
                }
            },
            move |journal| {
                journal.database.unit_of_work(|tx| {
                    let at = UtcTimestamp::now();
                    let projection =
                        inbox::submit_message(tx, command, &stored, &skills, &prepared.values, at)?;
                    if let (Some(rewrite), Some(message)) = (&rewrite, projection.messages.last()) {
                        let subject = message.message_id.to_string();
                        let event = aura_hooks::HookEvent::MessageBeforeAccept;
                        if aura_persistence_v2::hooks::decision(tx, &subject, event)?.is_none() {
                            aura_persistence_v2::hooks::save_decision(
                                tx, &subject, event, rewrite, at,
                            )?;
                        }
                    }
                    Ok::<_, aura_persistence_core::PersistenceCoreError>(projection)
                })
            },
        )
        .await?;
        self.schedule_inbox(request.session_id).await?;
        self.inbox(AgentSessionRequest { session_id: request.session_id }).await
    }

    /// The Skills chosen in the composer, checked off the reactor (it reads folders).
    pub(super) async fn selected_skills(
        &self,
        workspace: Option<WorkspaceId>,
        ids: &[SkillInstanceId],
    ) -> anyhow::Result<Vec<LoadedSkillEntry>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let runtime = crate::agent_execution::skill_tools::runtime(&self.execution)?;
        let ids = ids.to_vec();
        tokio::task::spawn_blocking(move || {
            crate::agent_execution::skill_tools::selected(&runtime, workspace, &ids)
        })
        .await?
    }

    pub async fn mutate_inbox(
        self: &Arc<Self>,
        command: CommandInvocationId,
        request: AgentInboxMutation,
    ) -> anyhow::Result<AgentInboxProjection> {
        self.ensure_session(request.session_id).await?;
        // An edited message is a new text: its Hooks decide again, and an old rewrite goes.
        let rewrite = match &request.action {
            AgentInboxAction::Edit { message_id, text } => {
                let workspace = self.session_workspace(request.session_id);
                Some((
                    *message_id,
                    self.accept_message(request.session_id, workspace, text, "edit").await?,
                ))
            }
            _ => None,
        };
        let boundary = self.execution.persistence.permission_change_lease(request.session_id).await;
        let stored = request.clone();
        self.store
            .write(StoreStep::operation("agent.inbox.mutate"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    let at = UtcTimestamp::now();
                    let projection = inbox::mutate(tx, command, &stored, at)?;
                    if let Some((message, decision)) = &rewrite {
                        aura_persistence_v2::hooks::replace_decision(
                            tx,
                            &message.to_string(),
                            aura_hooks::HookEvent::MessageBeforeAccept,
                            decision.as_ref(),
                            at,
                        )?;
                    }
                    Ok::<_, aura_persistence_core::PersistenceCoreError>(projection)
                })
            })
            .await?;
        drop(boundary);
        if matches!(request.action, AgentInboxAction::Resume) {
            self.schedule_inbox(request.session_id).await?;
        }
        self.inbox(AgentSessionRequest { session_id: request.session_id }).await
    }

    pub(super) async fn drain_inbox(self: &Arc<Self>, id: SessionId) -> anyhow::Result<()> {
        let pending = self
            .store
            .read(StoreStep::operation("agent.inbox.drain"), move |db| {
                if session::load(db, id)?.active_turn.is_some() {
                    return Ok(None);
                }
                let pending = inbox::load(db, id)?;
                if pending.paused
                    || !pending.messages.iter().any(|m| m.destination == AgentInputLane::NextTurn)
                {
                    return Ok(None);
                }
                Ok(Some(pending))
            })
            .await?;
        let Some(pending) = pending else {
            return Ok(());
        };
        let profile = self
            .execution
            .persistence
            .session_model_binding(id)?
            .ok_or_else(|| anyhow::anyhow!("agent.model_binding_required"))?
            .model_profile_id;
        let first = pending
            .messages
            .iter()
            .find(|m| m.destination == AgentInputLane::NextTurn)
            .ok_or_else(|| anyhow::anyhow!("agent.queue_empty"))?;
        let result = self
            .start_input(
                CommandInvocationId::from_bytes(first.message_id.to_bytes()),
                AgentStartRequest {
                    session_id: id,
                    model_profile_id: profile,
                    text: first.text.clone(),
                    mode: AgentTaskMode::Normal,
                    goal_budget: None,
                    explicit_backend_id: None,
                    skill_ids: Vec::new(),
                    // The turn takes in every queued message; the newest one's level is
                    // the user's latest choice.
                    reasoning_effort: pending
                        .messages
                        .iter()
                        .filter(|m| m.destination == AgentInputLane::NextTurn)
                        .filter_map(|m| m.reasoning_effort)
                        .next_back(),
                    attachments: Vec::new(),
                },
                Some(pending),
                None,
            )
            .await;
        match result {
            Ok(_) => Ok(()),
            // Another authenticated input can win the same start transaction.
            // It must not cause automatic replay or discard the remaining input.
            Err(_) => {
                let active = self
                    .store
                    .read(StoreStep::operation("agent.inbox.winner_check"), move |db| {
                        Ok(session::load(db, id)?.active_turn.is_some())
                    })
                    .await
                    .unwrap_or(false);
                if active { Ok(()) } else { result.map(|_| ()) }
            }
        }
    }

    pub(super) async fn schedule_inbox(self: &Arc<Self>, id: SessionId) -> anyhow::Result<()> {
        if let Err(error) = self.drain_inbox(id).await {
            let reason = error.to_string();
            // The queue's own pause fact is a run-local diagnostic of the session's scheduling
            // step: it is recorded on the session's queue, never as a store-wide fault.
            self.store
                .write(StoreStep::operation("agent.inbox.pause"), move |journal| {
                    journal.database.unit_of_work(|tx| {
                        inbox::pause_with_reason(tx, id, Some(reason), UtcTimestamp::now())
                    })
                })
                .await?;
        }
        Ok(())
    }
}
