//! Authenticated task creation; queued text is frozen only by consumption.
use super::*;
use aura_agent_execution::agent_execution;
use aura_core_broker::agent_store::StoreStep;
impl AgentApplication {
    pub async fn start(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentStartRequest,
    ) -> anyhow::Result<AgentRunProjection> {
        self.start_input(command, input, None, None).await
    }
    /// `/init`: a one-off workspace initialization task in an idle session. It runs as
    /// Normal with the session's permission mode and model, and its user message is the
    /// command with the user's note, exactly as the transcript shows it.
    pub async fn start_init(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentInitStartRequest,
    ) -> anyhow::Result<AgentRunProjection> {
        let note = input.text.trim();
        let text = if note.is_empty() { "/init".to_owned() } else { format!("/init\n\n{note}") };
        let request = AgentStartRequest {
            session_id: input.session_id,
            model_profile_id: input.model_profile_id,
            text,
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        };
        self.start_purpose(
            command,
            request,
            None,
            None,
            AgentTaskPurpose::WorkspaceInit,
            None,
            None,
        )
        .await
    }
    /// `/compact` in an idle session. Its
    /// user message is the command with the user's note, exactly as the transcript shows it; the
    /// compaction is requested for the turn's run as it is created, so Core compacts before the
    /// first step, and the model only confirms it.
    pub async fn start_compaction(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentCompactionStartRequest,
    ) -> anyhow::Result<AgentRunProjection> {
        let focus = input.focus.as_deref().map(str::trim).filter(|note| !note.is_empty());
        let text =
            focus.map_or_else(|| "/compact".to_owned(), |note| format!("/compact\n\n{note}"));
        let request = AgentStartRequest {
            session_id: input.session_id,
            model_profile_id: input.model_profile_id,
            text,
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        };
        let focus = focus.map(str::to_owned);
        self.start_purpose(
            command,
            request,
            None,
            None,
            AgentTaskPurpose::Compaction,
            Some(focus),
            None,
        )
        .await
    }
    pub(super) async fn start_input(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentStartRequest,
        inbox: Option<AgentInboxProjection>,
        related: Option<AgentRelatedTurnRequest>,
    ) -> anyhow::Result<AgentRunProjection> {
        self.start_purpose(
            command,
            input,
            inbox,
            related,
            AgentTaskPurpose::Conversation,
            None,
            None,
        )
        .await
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn start_purpose(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentStartRequest,
        inbox: Option<AgentInboxProjection>,
        related: Option<AgentRelatedTurnRequest>,
        purpose: AgentTaskPurpose,
        // A compaction task's request: the user's focus, if any.
        compaction: Option<Option<String>>,
        // A "继续" for a turn a restart cut off: it goes ahead of the queue.
        continuation: Option<super::continuation::Continuation>,
    ) -> anyhow::Result<AgentRunProjection> {
        // A command is compared by its receipt, which holds its attachments' digests.
        let existing = input.receipt();
        let previous = self
            .store
            .read(StoreStep::operation("agent.start.submitted"), move |db| {
                store::submitted(db, command, &existing)
            })
            .await?;
        if let Some(run) = previous {
            return Ok(self
                .status(&AgentRunRequest { session_id: input.session_id, agent_run_id: run })
                .await?);
        }
        anyhow::ensure!(
            (!input.text.trim().is_empty() || !input.attachments.is_empty())
                && input.text.len() <= 256 * 1024
                && (inbox.is_none() || input.attachments.is_empty())
                && (input.mode == AgentTaskMode::Goal || input.goal_budget.is_none())
                && input
                    .explicit_backend_id
                    .as_deref()
                    .is_none_or(|id| id == aura_backend_windows_user::NATIVE_BACKEND_ID),
            "agent.submission_invalid"
        );
        let config = self.p05.model_config.snapshot()?;
        anyhow::ensure!(
            config.registry.models.get(&input.model_profile_id).is_some_and(|m| m.enabled),
            "model.profile_unavailable"
        );
        let projection = self.execution.persistence.workspace_session_projection()?;
        let session = projection
            .sessions
            .iter()
            .find(|s| s.session_id == input.session_id)
            .ok_or_else(|| anyhow::anyhow!("agent.session_missing"))?;
        let workspace = projection
            .workspaces
            .iter()
            .find(|w| w.workspace_id == session.workspace_id)
            .ok_or_else(|| anyhow::anyhow!("agent.workspace_missing"))?;
        let message_rewrite = if inbox.is_none() {
            self.accept_message(input.session_id, Some(session.workspace_id), &input.text, "start")
                .await?
        } else {
            None
        };
        // The pictures and files sent with a message that starts the turn itself; a queued
        // message's were kept when it was queued.
        let attachments::PreparedAttachments { values: prepared, grants } = self
            .prepare_attachments(
                command,
                input.session_id,
                Some(session.workspace_id),
                Some(input.model_profile_id),
                &input.attachments,
            )
            .await?;
        let at = UtcTimestamp::now();
        // The root message as it becomes part of the conversation, with what the user typed and
        // the attachments recorded with it.
        let (user, typed, attached) = if let Some(pending) = &inbox {
            let root = pending
                .messages
                .iter()
                .find(|m| m.destination == AgentInputLane::NextTurn)
                .ok_or_else(|| anyhow::anyhow!("agent.queue_empty"))?
                .clone();
            let user = if root.attachments.is_empty() {
                root.freeze(input.session_id, at)?
            } else {
                let (message, session) = (root.clone(), input.session_id);
                self.store
                    .read(StoreStep::operation("agent.start.attachments"), move |db| {
                        aura_persistence_v2::agent::attachments::freeze(db, &message, session, at)
                    })
                    .await?
            };
            (user, root.text, Ok(root.attachments))
        } else {
            let parts = prepared.iter().map(|attachment| attachment.part()).collect::<Vec<_>>();
            let user = OriginalUserMessage::new(
                EventId::new(),
                input.session_id,
                aura_agent_domain::message_with_attachments(&input.text, &parts),
                UserMessageOrigin::UserInput,
                at,
            )?;
            (user, input.text.clone(), Err(prepared))
        };
        let bundle = aura_prompt_runtime::freeze_prompt_bundle(
            &aura_prompt_runtime::ValidatedPromptBundle::embedded()?,
            &self.settings.prompt_snapshot()?,
            TurnTaskId::new(),
            env!("CARGO_PKG_VERSION"),
            at,
        )?;
        let tool_settings = self.settings.tool_settings_snapshot()?;
        // A queued message's Skills were checked when it was sent; a direct start checks
        // them now. Every message this turn takes in keeps its own choice for its history.
        let mut chosen: Vec<(EventId, Vec<LoadedSkillEntry>)> = match &inbox {
            Some(pending) => pending
                .messages
                .iter()
                .filter(|m| m.destination == AgentInputLane::NextTurn && !m.skills.is_empty())
                .map(|m| (m.message_id, m.skills.clone()))
                .collect(),
            None => vec![(
                user.event_id,
                self.selected_skills(Some(session.workspace_id), &input.skill_ids).await?,
            )],
        };
        chosen.retain(|(_, skills)| !skills.is_empty());
        let mut loaded: Vec<LoadedSkillEntry> = Vec::new();
        for skill in chosen.iter().flat_map(|(_, skills)| skills) {
            if loaded.len() < agent_execution::skill_tools::MAX_LOADED
                && !loaded.iter().any(|entry| entry.skill_id == skill.skill_id)
            {
                loaded.push(skill.clone());
            }
        }
        let workspace = workspace.clone();
        let session = session.clone();
        let owned = input.clone();
        let inbox_owned = inbox.clone();
        let related_owned = related.clone();
        let repeated = owned.receipt();
        let (turn, created) = self
            .accept_attachments(
                "agent.start.create_turn",
                input.session_id,
                grants,
                move |db| {
                    store::submitted(db, command, &repeated)?
                        .map(|run| {
                            Ok((turns::load(db, turns::load_run(db, run)?.turn_task_id)?, false))
                        })
                        .transpose()
                },
                move |journal| {
                    journal.database.unit_of_work(|tx| {
                        if let Some(run) = store::submitted(tx, command, &owned)? {
                            return Ok((
                                turns::load(tx, turns::load_run(tx, run)?.turn_task_id)?,
                                false,
                            ));
                        }
                        AgentApplication::mirror_session(tx, &workspace, &session)?;
                        if purpose.is_command() {
                            let current =
                                aura_persistence_v2::agent::session::load(tx, owned.session_id)?;
                            if current.active_turn.is_some()
                                || !current.pending_user_event_ids.is_empty()
                                || !aura_persistence_v2::agent::inbox::load(tx, owned.session_id)?
                                    .messages
                                    .is_empty()
                            {
                                return Err(aura_persistence_core::PersistenceCoreError::Invalid(
                                    if purpose == AgentTaskPurpose::Compaction {
                                        "agent.compaction_start_requires_idle"
                                    } else {
                                        "agent.init_requires_idle"
                                    },
                                ));
                            }
                        }
                        if owned.mode != AgentTaskMode::Normal {
                            let current =
                                aura_persistence_v2::agent::session::load(tx, owned.session_id)?;
                            if current.active_turn.is_some() {
                                return Err(aura_persistence_core::PersistenceCoreError::Invalid(
                                    "agent.mode_switch_requires_idle",
                                ));
                            }
                        }
                        if inbox_owned.is_none()
                            && related_owned.is_none()
                            && continuation.is_none()
                            && !aura_persistence_v2::agent::inbox::load(tx, session.session_id)?
                                .messages
                                .is_empty()
                        {
                            return Err(aura_persistence_core::PersistenceCoreError::Invalid(
                                "agent.queue_must_be_resolved_before_mode_start",
                            ));
                        }
                        for a in &bundle.artifacts {
                            aura_persistence_core::artifact::put(
                                tx,
                                a.artifact_id,
                                &bundle.snapshot.id.to_string(),
                                &a.media_type,
                                &a.bytes,
                                at,
                            )?;
                        }
                        aura_persistence_v2::context::insert_bundle(
                            tx,
                            owned.session_id,
                            &bundle.snapshot,
                        )?;
                        let turn = lifecycle::start_for(
                            tx,
                            &user,
                            owned.mode,
                            owned.goal_budget.clone(),
                            bundle.snapshot.id,
                            purpose,
                        )?;
                        // A queued root message's attachments were kept when it was queued
                        // (Ok); a started message's are kept now (Err holds them as prepared).
                        let kept = match &attached {
                            Ok(kept) => kept.clone(),
                            Err(prepared) => aura_persistence_v2::agent::attachments::keep(
                                tx,
                                owned.session_id,
                                prepared,
                                at,
                            )?,
                        };
                        aura_persistence_v2::agent::attachments::record(
                            tx,
                            user.event_id,
                            &typed,
                            &kept,
                            at,
                        )?;
                        if let Some(rewrite) = &message_rewrite {
                            aura_persistence_v2::hooks::save_decision(
                                tx,
                                &user.event_id.to_string(),
                                aura_hooks::HookEvent::MessageBeforeAccept,
                                rewrite,
                                at,
                            )?;
                        }
                        let mut recorded = owned.receipt();
                        recorded.mode = turn.mode;
                        if turn.mode != AgentTaskMode::Goal {
                            recorded.goal_budget = None;
                        }
                        if let Some(pending) = &inbox_owned {
                            aura_persistence_v2::agent::input_consumption::start_batch(
                                tx, pending, &turn, at,
                            )?;
                        }
                        if let Some(source) = &related_owned {
                            aura_persistence_v2::agent::related::record(
                                tx, command, source, &turn, at,
                            )?;
                        }
                        if let Some(continuation) = &continuation {
                            continuation.record(tx, &turn, at)?;
                        }
                        if !loaded.is_empty() {
                            let run = turn.agent_run_id;
                            let ids: Vec<_> = loaded.iter().map(|skill| skill.skill_id).collect();
                            aura_persistence_v2::agent::skills::set_loaded(tx, run, &loaded, at)?;
                            aura_persistence_v2::agent::skills::set_chosen(tx, run, &ids, at)?;
                        }
                        for (message, skills) in &chosen {
                            aura_persistence_v2::agent::skills::set_selected(
                                tx, *message, skills, at,
                            )?;
                        }
                        store::configure(
                            tx,
                            turn.agent_run_id,
                            &store::RunConfiguration { request: recorded, tool_settings, purpose },
                            command,
                            at,
                        )?;
                        if let Some(focus) = &compaction {
                            aura_persistence_v2::context::manual_compaction::request(
                                tx,
                                CommandInvocationId::new(),
                                &aura_context_domain::AgentCompactionRequest {
                                    session_id: owned.session_id,
                                    agent_run_id: turn.agent_run_id,
                                    focus: focus.clone(),
                                },
                                at,
                            )?;
                        }
                        Ok::<_, aura_persistence_core::PersistenceCoreError>((turn, true))
                    })
                },
            )
            .await?;
        if created {
            self.launch(turn.agent_run_id)?;
        }
        Ok(self
            .status(&AgentRunRequest {
                session_id: input.session_id,
                agent_run_id: turn.agent_run_id,
            })
            .await?)
    }
}
