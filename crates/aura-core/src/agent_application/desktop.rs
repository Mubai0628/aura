//! Read-only Agent entry projections, on the bounded Store owner.
//!
//! Every one of these is a real Main entry (`agent.history`, `agent.result`, `agent.inspector`,
//! `agent.message.text`, `agent.compaction.*`, `agent.query.select_file`, `agent.related.start`,
//! and `agent.tool_activity`),
//! and each used to take the journal mutex and run SQLite on the reactor thread that also has to
//! deliver the heartbeat. They now run as bounded Store sections like every other Agent step; the
//! values they return and the session binding they check are unchanged.
use super::*;
use crate::agent_store::StoreStep;

impl AgentApplication {
    /// A
    /// conversation's tool rows were read under the journal mutex on the reactor thread, holding
    /// every write of the store while a long conversation's rows were read. They are read as a
    /// bounded section on the store's reader, like the other entries here.
    pub async fn tool_activity(
        &self,
        input: aura_tool_projection::AgentToolActivityRequest,
    ) -> anyhow::Result<aura_tool_projection::AgentToolActivityPage> {
        Ok(self
            .store
            .read(StoreStep::operation("agent.tool_activity"), move |db| {
                aura_persistence_v2::tool::activity::page(db, &input)
            })
            .await?)
    }

    pub async fn request_compaction(
        &self,
        command: CommandInvocationId,
        input: aura_context_domain::AgentCompactionRequest,
    ) -> anyhow::Result<aura_context_domain::AgentCompactionStatus> {
        Ok(self
            .store
            .write(StoreStep::operation("agent.compaction.request"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    aura_persistence_v2::context::manual_compaction::request(
                        tx,
                        command,
                        &input,
                        UtcTimestamp::now(),
                    )
                })
            })
            .await?)
    }

    pub async fn compaction_status(
        &self,
        input: AgentRunRequest,
    ) -> anyhow::Result<aura_context_domain::AgentCompactionStatus> {
        Ok(self
            .store
            .read(StoreStep::run(input.agent_run_id, "agent.compaction.status"), move |db| {
                aura_persistence_v2::context::manual_compaction::status(db, &input)
            })
            .await?)
    }

    pub async fn query_file(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentQueryFileRequest,
    ) -> anyhow::Result<AgentQueryFileSelection> {
        let stored = input.clone();
        let previous = self
            .store
            .read(StoreStep::run(input.agent_run_id, "agent.query.selected_file"), move |db| {
                aura_persistence_v2::agent::query::selected_file(db, command, &stored)
            })
            .await?;
        if let Some(selected) = previous {
            return Ok(selected);
        }
        let current = self
            .status(&AgentRunRequest {
                session_id: input.session_id,
                agent_run_id: input.agent_run_id,
            })
            .await?;
        let query = current.pending_query.ok_or_else(|| anyhow::anyhow!("agent.query_missing"))?;
        anyhow::ensure!(
            query.query_id == input.query_id
                && query.revision == input.expected_revision
                && query.state == UserQueryState::Pending
                && query.input.questions.iter().any(|q| q.id == input.question_id
                    && q.answer_type == QueryAnswerType::FileOrDirectory),
            "agent.query_changed"
        );
        let path = WorkspacePath::parse(&input.relative_path)?;
        self.execution.fs.stat(aura_workspace_domain::FsStatRequest {
            workspace_id: current.turn.workspace_id,
            session_id: input.session_id,
            target: aura_workspace_domain::WorkspaceFileTarget { path: path.clone() },
        })?;
        let selected = AgentQueryFileSelection {
            file_reference_id: self.execution.persistence.create_file_reference(
                input.session_id,
                current.turn.workspace_id,
                &path,
            )?,
            relative_path: path.to_string(),
        };
        let stored = input.clone();
        let selection = selected.clone();
        Ok(self
            .store
            .write(StoreStep::run(input.agent_run_id, "agent.query.select_file"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    aura_persistence_v2::agent::query::select_file(
                        tx,
                        command,
                        &stored,
                        &selection,
                        UtcTimestamp::now(),
                    )
                })
            })
            .await?)
    }

    pub async fn related_turn(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentRelatedTurnRequest,
    ) -> anyhow::Result<AgentRunProjection> {
        self.ensure_session(input.session_id).await?;
        let stored = input.clone();
        let previous = self
            .store
            .read(StoreStep::operation("agent.related.submitted"), move |db| {
                aura_persistence_v2::agent::related::submitted(db, command, &stored)
            })
            .await?;
        if let Some(run) = previous {
            return Ok(self
                .status(&AgentRunRequest { session_id: input.session_id, agent_run_id: run })
                .await?);
        }
        let stored = input.clone();
        let start = self
            .store
            .read(StoreStep::operation("agent.related.validate"), move |db| {
                aura_persistence_v2::agent::related::validate(db, &stored)
            })
            .await?;
        let profile = self
            .execution
            .persistence
            .session_model_binding(input.session_id)?
            .ok_or_else(|| anyhow::anyhow!("agent.model_binding_required"))?
            .model_profile_id;
        self.start_input(
            command,
            AgentStartRequest {
                session_id: input.session_id,
                model_profile_id: profile,
                text: start.text,
                mode: start.mode,
                goal_budget: start.goal_budget,
                explicit_backend_id: None,
                skill_ids: Vec::new(),
                reasoning_effort: None,
                attachments: Vec::new(),
            },
            None,
            Some(input),
        )
        .await
    }

    pub async fn inspector(
        &self,
        input: AgentInspectorRequest,
    ) -> anyhow::Result<aura_context_domain::AgentPromptInspection> {
        Ok(self
            .store
            .read(StoreStep::operation("agent.inspector"), move |db| {
                aura_persistence_v2::agent::inspector::read(db, &input)
            })
            .await?)
    }

    pub async fn history(
        self: &Arc<Self>,
        input: AgentHistoryRequest,
    ) -> anyhow::Result<AgentSessionProjection> {
        self.ensure_session(input.session_id).await?;
        let runtime = self.execution.runtime;
        // What running steps are writing into tool calls right now.
        let live = super::tool_input::snapshot();
        Ok(self
            .store
            .read(StoreStep::operation("agent.history"), move |db| {
                let mut projection =
                    aura_persistence_v2::agent::desktop::projection(db, &input, runtime)?;
                // The newest page is the one the desktop polls and compares; an earlier page is
                // merged into it and keeps the store's revision.
                if input.before_turn_task_id.is_none() && input.within_turn.is_none() {
                    super::history_revision::settle(&mut projection, input.limit)?;
                }
                aura_persistence_v2::agent::desktop::with_tool_input(db, &mut projection, &live)?;
                Ok(projection)
            })
            .await?)
    }

    pub async fn result(
        self: &Arc<Self>,
        input: AgentResultRequest,
    ) -> anyhow::Result<AgentResultView> {
        self.ensure_session(input.session_id).await?;
        Ok(self
            .store
            .read(StoreStep::operation("agent.result"), move |db| {
                aura_persistence_v2::agent::desktop::result(db, &input)
            })
            .await?)
    }

    pub async fn message_text(
        &self,
        input: AgentMessageTextRequest,
    ) -> anyhow::Result<AgentMessageTextPage> {
        Ok(self
            .store
            .read(StoreStep::operation("agent.message.text"), move |db| {
                aura_persistence_v2::agent::desktop_messages::text(db, &input)
            })
            .await?)
    }
}
