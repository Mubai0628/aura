use super::*;
use aura_core_broker::agent_store::{StoreAccess, StoreStep};
use aura_execution_contract::BackendDescriptor;
use aura_permission_domain::SessionPermissionProjection;
use aura_persistence_core::{StoreResult, Transaction};
use aura_persistence_v2::agent::approval;
use aura_security_contract::{AuthorizationProposal, PermissionMode};

impl AgentApplication {
    /// Reconciles managed calls and pending approvals with the new permission mode.
    pub async fn mode_changed(
        self: &Arc<Self>,
        session: SessionId,
        before: PermissionMode,
    ) -> anyhow::Result<()> {
        let deadline = Self::stop_deadline();
        let stopped = self
            .execution
            .stop_authority_dependencies(
                session,
                Some(before),
                Some(self.commands.clone()),
                deadline,
            )
            .await;
        self.resume_stale_approval(session, deadline).await?;
        stopped
    }

    /// Returns true if the recorded approval still needs a user answer.
    pub(super) async fn pause_for_approval(
        self: &Arc<Self>,
        request: AgentRunRequest,
        batch: ToolBatchId,
        call: ToolCallId,
        proposal: AuthorizationProposal,
        backend: BackendDescriptor,
    ) -> anyhow::Result<bool> {
        let run = request.agent_run_id;
        let session = request.session_id;
        self.write_approval_at_live_mode(
            session,
            StoreStep::run(run, "agent.drive.approval_pause"),
            move |tx, current| {
                let at = UtcTimestamp::now();
                let expires = UtcTimestamp::from_value(at.value() + time::Duration::minutes(10));
                approval::pause(tx, run, batch, call, proposal, backend, at, expires)?;
                // A mode change may have landed while the tool batch was preparing its answer.
                // Commit the closed state with the refusal so no stale card can be published.
                Ok(approval::invalidate_mode(tx, session, current.mode, current.revision, at)?
                    .is_none())
            },
        )
        .await
    }

    pub(super) async fn resume_stale_approval(
        self: &Arc<Self>,
        session: SessionId,
        deadline: Instant,
    ) -> anyhow::Result<bool> {
        let run = self
            .write_approval_at_live_mode(
                session,
                StoreStep::operation("agent.approval.invalidate_mode"),
                move |tx, current| {
                    approval::invalidate_mode(
                        tx,
                        session,
                        current.mode,
                        current.revision,
                        UtcTimestamp::now(),
                    )
                },
            )
            .await?;
        let Some(run) = run else { return Ok(false) };
        self.idle_within(run, deadline).await?;
        self.expiry_timers.cancel(run);
        self.launch(run)?;
        Ok(true)
    }

    async fn write_approval_at_live_mode<T: Send + 'static>(
        self: &Arc<Self>,
        session: SessionId,
        step: StoreStep,
        write: impl FnOnce(&Transaction<'_>, &SessionPermissionProjection) -> StoreResult<T>
        + Send
        + 'static,
    ) -> anyhow::Result<T> {
        let persistence = Arc::clone(&self.execution.persistence);
        let journal = Arc::clone(&self.execution.journal);
        self.store
            .run_critical(step, StoreAccess::Write, move || {
                persistence.with_session_permission_authority(
                    session,
                    |current| -> anyhow::Result<T> {
                        let mut store = journal.lock()?;
                        Ok(store.database.unit_of_work(|tx| write(tx, current))?)
                    },
                )?
            })
            .await
    }

    pub async fn file_authority_withdrawn(
        self: &Arc<Self>,
        session: SessionId,
    ) -> anyhow::Result<()> {
        self.execution
            .stop_authority_dependencies(
                session,
                None,
                Some(self.commands.clone()),
                Self::stop_deadline(),
            )
            .await
    }
}
