//! A sub-agent's questions answered before or instead of the user. Its main agent answers first
//! those the settings let it (agents.approve) while its turn is under way, for at most
//! `REVIEW_SECONDS` after the question was asked; the settings answer its downloads when they
//! allow them. Each answer covers one call, except the main agent's allow_workspace, which is the
//! allowance the user's card offers; everything else waits for the user as before.
use super::*;
use aura_agent_team::{
    Answered, AnsweredBy, ApprovalAnswer, REVIEW_SECONDS, ReviewRequest, note_review,
};
use aura_execution_contract::{
    ExecutionApprovalRequest, ExecutionApprovalResolutionRequest, ExecutionApprovalView,
};
use aura_persistence_v2::agent::{approval, approver, session};
use aura_security_contract::{ApprovalDecisionKind, ResolvedCapability};

/// What a sub-agent's settings answer is recorded with.
const DOWNLOAD_REASON: &str = "设置里允许子 Agent 下载。";

impl AgentApplication {
    /// Splits the questions sub-agent `conversation` waits on into the user's and those its main
    /// agent answers first.
    pub(super) async fn split_reviews(
        self: &Arc<Self>,
        conversation: SessionId,
        pending: Vec<ApprovalRequestId>,
    ) -> anyhow::Result<(Vec<ApprovalRequestId>, Vec<ReviewRequest>)> {
        if pending.is_empty() {
            return Ok((pending, Vec::new()));
        }
        let reviewer = self
            .store
            .read(StoreStep::operation("agents.review.reviewer"), move |db| {
                let Some(member) = team_store::member_by_child(db, conversation)? else {
                    return Ok(false);
                };
                let approves = aura_agent_team::limits::settings(db)?.1.main_agent_approves;
                // The main agent answers only while a turn of its own is under way.
                let working = session::load(db, member.parent_session_id)?.active_turn.is_some();
                Ok(approves && working)
            })
            .await?;
        if !reviewer {
            return Ok((pending, Vec::new()));
        }
        let now = UtcTimestamp::now();
        let window = time::Duration::seconds(i64::from(REVIEW_SECONDS));
        let (mut users, mut reviewing) = (Vec::new(), Vec::new());
        for approval_id in pending {
            let view = self
                .stored_approval(ExecutionApprovalRequest { session_id: conversation, approval_id })
                .await?;
            let asked_at = view.approval.requested_at;
            let reviewable = if now.value() - asked_at.value() < window {
                let execution = Arc::clone(&self.execution);
                let operation = view.operation.clone();
                tokio::task::spawn_blocking(move || execution.reviewable(&operation)).await??
            } else {
                None
            };
            let Some(reviewable) = reviewable else {
                users.push(approval_id);
                continue;
            };
            let allow_workspace = self.offers_deletion_allowance(&view).await?;
            let stored = view.clone();
            let purpose = self
                .store
                .read(StoreStep::operation("agents.review.purpose"), move |db| {
                    purpose_of(db, &stored)
                })
                .await?;
            reviewing.push(ReviewRequest {
                approval_id,
                tool: view.operation.content.action_kind.clone(),
                request: reviewable.request,
                purpose,
                deletes: reviewable.deletes,
                allow_workspace,
                asked_at,
            });
        }
        Ok((users, reviewing))
    }

    /// Answers sub-agent `child`'s question as its main agent chose: allow and deny cover that one
    /// call, and allow_workspace is the allowance the user's card offers beside them. A refusal
    /// tells the sub-agent that its main agent refused the call, and `reason`.
    pub(super) async fn answer_for_main_agent(
        self: &Arc<Self>,
        child: SessionId,
        approval_id: ApprovalRequestId,
        answer: ApprovalAnswer,
        reason: &str,
    ) -> anyhow::Result<()> {
        let decision = match answer {
            ApprovalAnswer::Allow => ApprovalDecisionKind::AllowOnce,
            ApprovalAnswer::AllowWorkspace => ApprovalDecisionKind::AllowWorkspace,
            ApprovalAnswer::Deny => {
                self.note_denial(child, approval_id, reason).await?;
                ApprovalDecisionKind::Deny
            }
            ApprovalAnswer::AskUser => return Ok(()),
        };
        self.answer_once(child, approval_id, decision).await
    }

    /// Writes the main agent's refusal as the result of the call it refuses, just before the
    /// refusal is resolved: the first result a call is given is the one kept, so the sub-agent
    /// reads who refused it and why rather than a refusal of the user's. Nothing is written once
    /// the call no longer waits for this answer.
    async fn note_denial(
        self: &Arc<Self>,
        child: SessionId,
        approval_id: ApprovalRequestId,
        reason: &str,
    ) -> anyhow::Result<()> {
        let notice = aura_agent_team::main_agent_denial(reason);
        self.store
            .write(StoreStep::operation("agents.approve.denied"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    let request = ExecutionApprovalRequest { session_id: child, approval_id };
                    let view = approval::view(tx, &request)?;
                    if view.approval.state != aura_security_contract::ApprovalState::Pending {
                        return Ok(());
                    }
                    let Some(run) = view.operation.content.identity.agent_run_id else {
                        return Ok(());
                    };
                    let AgentRunState::AwaitingApproval { tool_batch_id } =
                        turns::load_run(tx, run)?.state
                    else {
                        return Ok(());
                    };
                    let call = approver::call_of(tx, child, approval_id)?;
                    let stored = aura_persistence_v2::tool::execution::call(tx, call)?;
                    if stored.result.is_some() || stored.attempt_id.is_some() {
                        return Ok(());
                    }
                    aura_persistence_v2::tool::result::not_executed_with_notice(
                        tx,
                        tool_batch_id,
                        call,
                        aura_tool_contract::ToolNotExecutedReasonV1::PermissionDenied,
                        Some(notice.as_str()),
                        UtcTimestamp::now(),
                    )
                    .map(drop)
                })
            })
            .await?;
        Ok(())
    }

    async fn answer_once(
        self: &Arc<Self>,
        session_id: SessionId,
        approval_id: ApprovalRequestId,
        decision: ApprovalDecisionKind,
    ) -> anyhow::Result<()> {
        let view =
            self.stored_approval(ExecutionApprovalRequest { session_id, approval_id }).await?;
        let request = ExecutionApprovalResolutionRequest {
            session_id,
            approval_id,
            expected_revision: view.approval.revision,
            decision,
        };
        self.resolve_approval(CommandInvocationId::new(), request).await?;
        Ok(())
    }

    /// After run `run` paused for a question: a sub-agent's download that the settings allow is
    /// answered at once for that call. The rest waits as before.
    pub(in crate::agent_application) fn answer_paused(self: &Arc<Self>, run: AgentRunId) {
        let app = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = app.answer_downloads(run).await {
                let reason = format!("{error:#}").chars().take(600).collect::<String>();
                aura_core_base::append_core_log(
                    "agents.settings_answer_failed",
                    serde_json::json!({"reason": reason}),
                );
            }
        });
    }

    async fn answer_downloads(self: &Arc<Self>, run: AgentRunId) -> anyhow::Result<()> {
        let (session_id, allowed, pending) = self
            .store
            .read(StoreStep::run(run, "agents.settings_answer"), move |db| {
                let session_id =
                    turns::load(db, turns::load_run(db, run)?.turn_task_id)?.session_id;
                let allowed = team_store::member_by_child(db, session_id)?
                    .is_some_and(|member| member.details.allowed.downloads);
                Ok((session_id, allowed, approval::pending(db, run)?))
            })
            .await?;
        if !allowed {
            return Ok(());
        }
        for approval_id in pending {
            let view =
                self.stored_approval(ExecutionApprovalRequest { session_id, approval_id }).await?;
            if !download_only(&view) {
                continue;
            }
            self.answer_once(session_id, approval_id, ApprovalDecisionKind::AllowOnce).await?;
            let answered = Answered {
                by: AnsweredBy::Settings,
                answer: ApprovalAnswer::Allow,
                reason: DOWNLOAD_REASON.to_owned(),
                at: UtcTimestamp::now(),
            };
            self.store
                .write(StoreStep::operation("agents.settings_answer.record"), move |journal| {
                    journal.database.unit_of_work(|tx| {
                        let Some(current) = team_store::member_by_child(tx, session_id)? else {
                            return Ok(());
                        };
                        let mut next = current.clone();
                        note_review(&mut next.details.reviews, approval_id, |review| {
                            review.answered = Some(answered);
                        });
                        next.revision += 1;
                        next.updated_at = UtcTimestamp::now();
                        team_store::save_member(tx, &current, &next)
                    })
                })
                .await?;
        }
        Ok(())
    }
}

/// Whether the question in `view` asks only for a download of a command: no risk flag, no
/// program to run, and what it asks for is the download and its network.
fn download_only(view: &ExecutionApprovalView) -> bool {
    let content = &view.operation.content;
    aura_security_contract::built_in_shell(&content.source, &content.action_kind)
        && content.risks.is_empty()
        && content.capabilities.contains(&ResolvedCapability::DownloadArtifact)
        && !content.capabilities.contains(&ResolvedCapability::DownloadWindowsExecutable)
        && view.approval.allowed_decisions.contains(&ApprovalDecisionKind::AllowOnce)
        && view.requested_capabilities.as_ref().is_some_and(|asked| {
            !asked.is_empty()
                && asked.iter().all(|capability| {
                    matches!(
                        capability,
                        ResolvedCapability::OrdinaryNetwork | ResolvedCapability::DownloadArtifact
                    )
                })
        })
}

/// What the sub-agent said the call in `view` is for, from the call it waits to run.
fn purpose_of(
    db: &aura_persistence_core::Connection,
    view: &ExecutionApprovalView,
) -> aura_persistence_core::StoreResult<Option<String>> {
    let Some(run) = view.operation.content.identity.agent_run_id else { return Ok(None) };
    let AgentRunState::AwaitingApproval { tool_batch_id } = turns::load_run(db, run)?.state else {
        return Ok(None);
    };
    let Some(admitted) = aura_persistence_v2::tool::admission::load(db, tool_batch_id)? else {
        return Ok(None);
    };
    let fingerprint = &view.operation.fingerprint;
    let Some(call) =
        admitted.operations.iter().find(|(_, operation)| operation.fingerprint == *fingerprint)
    else {
        return Ok(None);
    };
    let stored = aura_persistence_v2::tool::execution::call(db, *call.0)?;
    Ok(match stored.call.input {
        aura_tool_contract::NormalizedToolCallInputStateV1::Ready { purpose, .. } => {
            Some(purpose.as_str().to_owned())
        }
        aura_tool_contract::NormalizedToolCallInputStateV1::InvalidInput { .. } => None,
    })
}
