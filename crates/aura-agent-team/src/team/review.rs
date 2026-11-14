//! The main agent answering its sub-agents' questions first: agents.wait and agents.status give it
//! the questions under review, agents.wait wakes for one it has not been given, and agents.approve
//! answers one. Which questions are under review the host says: the settings let the main agent
//! answer, the question only deletes or writes in the sub-agent's own folders, and it was asked
//! less than [`crate::REVIEW_SECONDS`] ago while the main agent works.
use super::*;
use crate::{
    AgentApprovalRequestView, AgentsApproveInput, AgentsApproveResult, Answered, AnsweredBy,
    ApprovalAnswer, REVIEW_SECONDS, ReviewRequest, note_review,
};

/// The longest request text the main agent is given.
const REQUEST_CHARS: usize = 2_000;

impl<H: TeamHost> Team<'_, H> {
    /// The questions of `parent`'s sub-agents under review, each with the member it is of.
    async fn under_review(
        &self,
        parent: SessionId,
    ) -> anyhow::Result<Vec<(Member, ReviewRequest)>> {
        let members = self
            .host
            .read("agents.review.members", move |db| store::members_of(db, parent))
            .await?;
        let mut found = Vec::new();
        for member in members {
            let progress = self.host.progress(member.child_session_id).await?;
            let handed = |request: &ReviewRequest| {
                member.details.reviews.iter().any(|review| {
                    review.approval_id == request.approval_id && review.answered.is_some()
                })
            };
            for request in progress.reviewing {
                if !handed(&request) {
                    found.push((member.clone(), request));
                }
            }
        }
        Ok(found)
    }

    /// Whether a question under review has not been given to the main agent yet.
    pub(super) async fn review_not_given(&self, parent: SessionId) -> anyhow::Result<bool> {
        Ok(self.under_review(parent).await?.iter().any(|(member, request)| {
            !member.details.reviews.iter().any(|review| {
                review.approval_id == request.approval_id && review.presented_at.is_some()
            })
        }))
    }

    /// The questions under review as the main agent is given them, noted as given.
    pub(super) async fn give_reviews(
        &self,
        parent: SessionId,
    ) -> anyhow::Result<Vec<AgentApprovalRequestView>> {
        let found = self.under_review(parent).await?;
        let now = UtcTimestamp::now();
        let given: Vec<_> = found
            .iter()
            .map(|(member, request)| (member.child_session_id, request.approval_id))
            .collect();
        if !given.is_empty() {
            self.host
                .write("agents.review.given", move |tx| {
                    for (child, approval) in given {
                        let Some(current) = store::member_by_child(tx, child)? else { continue };
                        let mut next = current.clone();
                        note_review(&mut next.details.reviews, approval, |review| {
                            review.presented_at.get_or_insert(now);
                        });
                        if next.details.reviews != current.details.reviews {
                            next.revision += 1;
                            next.updated_at = now;
                            store::save_member(tx, &current, &next)?;
                        }
                    }
                    Ok(())
                })
                .await?;
        }
        Ok(found
            .into_iter()
            .map(|(member, request)| {
                let elapsed = (now.value() - request.asked_at.value()).whole_seconds();
                let left = i64::from(REVIEW_SECONDS).saturating_sub(elapsed).max(0);
                AgentApprovalRequestView {
                    agent: member.handle,
                    approval_id: request.approval_id,
                    tool: request.tool,
                    request: request.request.chars().take(REQUEST_CHARS).collect(),
                    purpose: request.purpose,
                    deletes: request.deletes,
                    allow_workspace: request.allow_workspace,
                    seconds_left: u32::try_from(left).unwrap_or(0),
                }
            })
            .collect())
    }

    pub(super) async fn approve(&self, call: &TeamCall) -> anyhow::Result<AgentsApproveResult> {
        let input: AgentsApproveInput = parse(&call.arguments)?;
        self.refuse_subagent(call.session_id).await?;
        let reason = input.reason.trim().to_owned();
        if reason.is_empty() || reason.chars().count() > 500 {
            return Err(Refusal::new(
                "agents.approve_invalid",
                "reason 写一句给用户看的理由，500 字以内。",
            )
            .into());
        }
        let member = self.one(call.session_id, &input.agent).await?;
        let reviewed =
            self.under_review(call.session_id).await?.into_iter().find_map(|(of, request)| {
                (of.child_session_id == member.child_session_id
                    && request.approval_id == input.approval_id)
                    .then_some(request)
            });
        let Some(request) = reviewed else {
            return Err(Refusal::new(
                "agents.approval_not_reviewed",
                format!(
                    "{} 没有等你回答的这个请求：它已经回答过，或者已交给用户（超过 {REVIEW_SECONDS} 秒、你这一轮已结束，或设置不让你代批）。",
                    member.handle
                ),
            )
            .into());
        };
        if input.answer == ApprovalAnswer::AllowWorkspace && !request.allow_workspace {
            return Err(Refusal::new(
                "agents.approve_invalid",
                format!(
                    "{} 的这个请求不能用 allow_workspace：它不是只因为在自己的文件夹里删除才问。请用 allow、deny 或 ask_user。",
                    member.handle
                ),
            )
            .into());
        }
        let child = member.child_session_id;
        // A refusal carries its reason: the sub-agent reads who refused its call and why.
        if input.answer != ApprovalAnswer::AskUser
            && let Err(error) =
                self.host.answer_approval(child, input.approval_id, input.answer, &reason).await
        {
            return Err(anyhow::anyhow!(super::failure::answer_failed(&member.handle, &error)));
        }
        let (approval, answer) = (input.approval_id, input.answer);
        let answered =
            Answered { by: AnsweredBy::MainAgent, answer, reason, at: UtcTimestamp::now() };
        let recorded = self
            .host
            .write("agents.approve.record", move |tx| {
                let Some(current) = store::member_by_child(tx, child)? else { return Ok(()) };
                let mut next = current.clone();
                note_review(&mut next.details.reviews, approval, |review| {
                    review.answered = Some(answered);
                });
                next.revision += 1;
                next.updated_at = UtcTimestamp::now();
                store::save_member(tx, &current, &next)
            })
            .await;
        if let Err(error) = recorded {
            let said = super::failure::not_recorded(&member.handle, answer, &error);
            return Err(anyhow::anyhow!(said));
        }
        self.news.notify_waiters();
        Ok(AgentsApproveResult { agent: member.handle, approval_id: approval, answer })
    }
}
