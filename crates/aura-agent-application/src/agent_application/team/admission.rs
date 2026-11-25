use super::*;
use aura_context_domain::{ContextSourceRef, ModelEgressAuthorizationBasis, ModelEgressSourceKind};
use aura_context_runtime::ContextFragment;
use aura_domain_common::Revision;
use aura_persistence_core::{Connection, PersistenceCoreError, StoreResult};
use aura_security_contract::NetworkDataClass;

const SESSION_OFF: &str = "本会话未开启「允许子 Agent」，不能分配新的临时工作。";
const NO_CONCURRENCY: &str = "同时运行的子 Agent 数设为 0，不能开始新的临时工作。";
const STOPPED: &str = "子 Agent 已停止，不能开始新的临时工作。";

pub(super) fn new_work_block(
    db: &Connection,
    parent: SessionId,
) -> StoreResult<Option<&'static str>> {
    let policy = aura_agent_collaboration::policy::load(db, parent)?;
    Ok(if !policy.allow_proactive_temporary_agents {
        Some(SESSION_OFF)
    } else if aura_agent_team::limits::limits(db)?.0.max_running == 0 {
        Some(NO_CONCURRENCY)
    } else {
        None
    })
}

fn child_work_block(db: &Connection, child: SessionId) -> StoreResult<Option<&'static str>> {
    let Some(member) = team_store::member_by_child(db, child)? else { return Ok(None) };
    if member.state == aura_agent_team::MemberState::Stopped {
        return Ok(Some(STOPPED));
    }
    new_work_block(db, member.parent_session_id)
}

pub(in crate::agent_application) fn can_start(
    db: &Connection,
    session: SessionId,
) -> StoreResult<bool> {
    Ok(child_work_block(db, session)?.is_none())
}

pub(in crate::agent_application) fn require_start(
    db: &Connection,
    session: SessionId,
) -> StoreResult<()> {
    if let Some(reason) = child_work_block(db, session)? {
        return Err(PersistenceCoreError::Invalid(reason));
    }
    Ok(())
}

pub(in crate::agent_application) fn is_control_pause(error: &anyhow::Error) -> bool {
    error.chain().any(|error| {
        let is_pause = |reason: &str| matches!(reason, SESSION_OFF | NO_CONCURRENCY | STOPPED);
        matches!(error.downcast_ref::<StoreError>(), Some(StoreError::Failed { error: PersistenceCoreError::Invalid(reason), .. }) if is_pause(reason))
            || matches!(error.downcast_ref::<PersistenceCoreError>(), Some(PersistenceCoreError::Invalid(reason)) if is_pause(reason))
    })
}

impl AgentApplication {
    pub(in crate::agent_application) async fn preflight_temporary_start(
        &self,
        session: SessionId,
        purpose: AgentTaskPurpose,
        model: aura_domain_common::ModelProfileId,
    ) -> anyhow::Result<()> {
        if purpose != AgentTaskPurpose::Compaction {
            let child = self
                .store
                .read(StoreStep::operation("agents.start.preflight"), move |db| {
                    require_start(db, session)?;
                    Ok(team_store::member_by_child(db, session)?.is_some())
                })
                .await?;
            if child {
                self.require_temporary_model(model)?;
            }
            #[cfg(any(test, feature = "test-hooks"))]
            super::start_test_gate::wait(session).await;
        }
        Ok(())
    }

    pub(super) fn require_temporary_model(
        &self,
        profile: aura_domain_common::ModelProfileId,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.p05
                .model_config
                .snapshot()?
                .registry
                .models
                .get(&profile)
                .is_some_and(|model| model.enabled && model.available_for_subagents),
            "agents.model_unavailable: 当前模型未获准用于子 Agent。"
        );
        Ok(())
    }

    pub(in crate::agent_application) fn multi_agent_fragment(
        self: &Arc<Self>,
        session: SessionId,
    ) -> Boxed<'_, anyhow::Result<ContextFragment>> {
        // Keep the team projection's futures off the context preparation stack.
        Box::pin(self.collect_multi_agent_fragment(session))
    }

    async fn collect_multi_agent_fragment(
        self: &Arc<Self>,
        session: SessionId,
    ) -> anyhow::Result<ContextFragment> {
        let (policy, limits, revision, pending) = self
            .store
            .read(StoreStep::operation("agents.controls.snapshot"), move |db| {
                let (limits, permissions, revision) = aura_agent_team::limits::settings(db)?;
                let pending = if permissions.main_agent_approves {
                    unpresented_reviews(db, session)?
                } else {
                    Vec::new()
                };
                Ok((
                    aura_agent_collaboration::policy::load(db, session)?,
                    limits,
                    revision,
                    pending,
                ))
            })
            .await?;
        let mut waiting_for_review = Vec::new();
        for (child, handle, pending) in pending {
            let (_, reviews) = self.split_reviews(child, pending).await?;
            if !reviews.is_empty() {
                waiting_for_review.push(handle);
            }
        }
        let state_revision =
            Revision::new(revision.saturating_add(policy.revision).saturating_add(1));
        let model_available = self.team_offered();
        let available_slots =
            if policy.allow_proactive_temporary_agents && model_available && limits.max_running > 0
            {
                let host = TeamApp(Arc::clone(self));
                Team::new(&host, self.execution.team.news())
                    .available_slots(session, limits.max_running)
                    .await?
            } else {
                0
            };
        Ok(ContextFragment {
            source_ref: ContextSourceRef::RuntimeState {
                state_kind: "agent.multi_agent_controls.v1".into(),
                state_revision,
            },
            source_kind: ModelEgressSourceKind::AuraCore,
            data_class: NetworkDataClass::PublicContent,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: serde_json::to_string(&serde_json::json!({"multi_agent_controls": {
                "max_temporary_running":limits.max_running,
                "allow_proactive_temporary_agents":policy.allow_proactive_temporary_agents,
                "temporary_model_available":model_available,
                "temporary_slots_available":available_slots,
                "waiting_for_main_agent_review": waiting_for_review,
                "rule":"New temporary work needs this conversation's opt-in, a free slot, and an eligible model. Linked peers have separate eligibility. Manage existing work with the tools currently available."
            }}))?,
        })
    }

    pub(in crate::agent_application) async fn wake_temporary_queues(
        self: &Arc<Self>,
        parent: Option<SessionId>,
    ) -> anyhow::Result<()> {
        let sessions = self
            .store
            .read(StoreStep::operation("agents.queues.eligible"), move |db| {
                let mut ready = Vec::new();
                for session in aura_persistence_v2::agent::inbox::pending_sessions(db)? {
                    if team_store::member_by_child(db, session)?.is_some_and(|member| {
                        parent.is_none_or(|id| id == member.parent_session_id)
                    }) && can_start(db, session)?
                    {
                        ready.push(session);
                    }
                }
                Ok(ready)
            })
            .await?;
        for session in sessions {
            self.schedule_inbox(session).await?;
        }
        Ok(())
    }
}

fn unpresented_reviews(
    db: &Connection,
    parent: SessionId,
) -> StoreResult<Vec<(SessionId, String, Vec<ApprovalRequestId>)>> {
    use aura_persistence_v2::agent::{approval, session};
    let mut ready = Vec::new();
    for member in team_store::members_of(db, parent)? {
        let child = member.child_session_id;
        let Some(turn_id) = session::load(db, child)?.active_turn else { continue };
        let turn = turns::load(db, turn_id)?;
        if turn.state != TurnTaskState::AwaitingApproval {
            continue;
        }
        let mut ids = approval::pending(db, turn.agent_run_id)?;
        ids.retain(|id| {
            !member.details.reviews.iter().any(|review| {
                review.approval_id == *id
                    && (review.presented_at.is_some() || review.answered.is_some())
            })
        });
        if !ids.is_empty() {
            ready.push((child, member.handle, ids));
        }
    }
    Ok(ready)
}
