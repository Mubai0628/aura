//! Following the sub-agents: `agents.status`, `agents.wait`, `agents.result`, and the list the
//! desktop shows.
use super::*;
use crate::{AgentTeamLeadView, AgentTeamMemberView, AgentTeamView, LIST_SUMMARY_CHARS};
use aura_domain_common::ApprovalRequestId;
use std::time::Duration;

pub const WAIT_DEFAULT_SECONDS: u32 = 600;
pub const WAIT_MAX_SECONDS: u32 = 1_800;
const POLL: Duration = Duration::from_secs(1);
const ANSWER_PAGE_DEFAULT: u32 = 8_000;
const ANSWER_PAGE_MAX: u32 = 32_000;

impl<H: TeamHost> Team<'_, H> {
    pub(super) async fn status(&self, call: &TeamCall) -> anyhow::Result<AgentsStatusResult> {
        let input: AgentsStatusInput = parse(&call.arguments)?;
        self.refuse_subagent(call.session_id).await?;
        if let Some(detail) = input.approval_detail {
            return Ok(AgentsStatusResult {
                agents: Vec::new(),
                overlaps: Vec::new(),
                approvals: Vec::new(),
                approval_detail: Some(self.review_detail(call.session_id, detail).await?),
            });
        }
        let handles: Vec<String> = input.agent.into_iter().collect();
        let mut agents = Vec::new();
        for member in self.chosen(call.session_id, &handles).await? {
            let child = member.child_session_id;
            let (latest, worktree) = self
                .host
                .read("agents.status.report", move |db| {
                    Ok((store::latest_report(db, child)?, crate::worktree::worktree_of(db, child)?))
                })
                .await?;
            let progress = self.host.progress(child).await?;
            let reports = latest.as_ref().map_or(0, |report| report.sequence);
            agents.push(AgentStatusView {
                agent: member.handle.clone(),
                name: member.details.name.clone(),
                model: member.details.model_name.clone(),
                state: member.state,
                working: progress.working,
                tool: progress.tool,
                // Waiting for the user: its own questions, and those the main agent handed over.
                waiting_for_approval: !progress.approvals.is_empty()
                    || progress.reviewing.iter().any(|request| {
                        member.details.reviews.iter().any(|review| {
                            review.approval_id == request.approval_id && review.answered.is_some()
                        })
                    }),
                reports,
                unread_reports: reports.saturating_sub(member.details.reports_seen),
                last_report: latest.map(|report| report_view(&member.handle, report)),
                branch: worktree.map(|worktree| worktree.details.branch),
                progress: progress.progress,
            });
        }
        let overlaps = self.overlaps(call.session_id).await?;
        let approvals = self.give_reviews(call.session_id).await?;
        Ok(AgentsStatusResult { agents, overlaps, approvals, approval_detail: None })
    }

    /// Waits, before the call is admitted, until one of the sub-agents it waits for has a report
    /// not yet given to the main agent, none of them is running, or its time is up. The call
    /// itself then only takes what is there.
    pub async fn prewait(&self, parent: SessionId, input: &AgentsWaitInput) -> anyhow::Result<()> {
        let seconds =
            input.timeout_seconds.unwrap_or(WAIT_DEFAULT_SECONDS).clamp(1, WAIT_MAX_SECONDS);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(u64::from(seconds));
        loop {
            let notified = self.news.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            // A question for the main agent to answer first ends the wait too.
            if self.waited_for(parent, &input.agents).await?.is_none()
                || self.review_not_given(parent).await?
            {
                return Ok(());
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Ok(());
            }
            tokio::select! {
                () = &mut notified => {}
                () = tokio::time::sleep_until(deadline.min(now + POLL)) => {}
            }
        }
    }

    /// The members still to be waited for, or none when the wait is over.
    async fn waited_for(
        &self,
        parent: SessionId,
        handles: &[String],
    ) -> anyhow::Result<Option<Vec<Member>>> {
        let chosen = match self.chosen(parent, handles).await {
            Ok(chosen) => chosen,
            // A wrong handle is the call's own refusal; there is nothing to wait for.
            Err(_) => return Ok(None),
        };
        let mut waiting = Vec::new();
        for member in chosen {
            let child = member.child_session_id;
            let latest = self
                .host
                .read("agents.wait.check", move |db| store::latest_report(db, child))
                .await?;
            if latest.is_some_and(|report| report.sequence > member.details.reports_seen) {
                return Ok(None);
            }
            if member.state == MemberState::Running {
                waiting.push(member);
            }
        }
        Ok((!waiting.is_empty()).then_some(waiting))
    }

    pub(super) async fn wait(&self, call: &TeamCall) -> anyhow::Result<AgentsWaitResult> {
        let input: AgentsWaitInput = parse(&call.arguments)?;
        self.refuse_subagent(call.session_id).await?;
        let chosen = self.chosen(call.session_id, &input.agents).await?;
        let children: Vec<SessionId> =
            chosen.iter().map(|member| member.child_session_id).collect();
        let taken = self
            .host
            .write("agents.wait.take", move |tx| {
                let mut taken = Vec::new();
                for child in children {
                    let Some(current) = store::member_by_child(tx, child)? else { continue };
                    let fresh = store::reports_after(tx, child, current.details.reports_seen)?;
                    if let Some(last) = fresh.last() {
                        let mut next = current.clone();
                        next.details.reports_seen = last.sequence;
                        next.revision += 1;
                        next.updated_at = UtcTimestamp::now();
                        store::save_member(tx, &current, &next)?;
                    }
                    taken.extend(fresh.into_iter().map(|report| (current.handle.clone(), report)));
                }
                Ok(taken)
            })
            .await?;
        let reported: Vec<&str> = taken.iter().map(|(handle, _)| handle.as_str()).collect();
        let mut working = Vec::new();
        let mut progress = std::collections::BTreeMap::new();
        for member in chosen
            .iter()
            .filter(|member| member.state == MemberState::Running)
            .filter(|member| !reported.contains(&member.handle.as_str()))
        {
            working.push(member.handle.clone());
            if let Some(view) = self.host.progress(member.child_session_id).await?.progress {
                progress.insert(member.handle.clone(), view);
            }
        }
        let approvals = self.give_reviews(call.session_id).await?;
        Ok(AgentsWaitResult {
            timed_out: taken.is_empty() && approvals.is_empty(),
            reports: taken
                .into_iter()
                .map(|(handle, report)| report_view(&handle, report))
                .collect(),
            working,
            progress,
            overlaps: self.overlaps(call.session_id).await?,
            approvals,
        })
    }

    pub(super) async fn result(&self, call: &TeamCall) -> anyhow::Result<AgentsResultResult> {
        let input: AgentsResultInput = parse(&call.arguments)?;
        self.refuse_subagent(call.session_id).await?;
        let member = self.one(call.session_id, &input.agent).await?;
        let child = member.child_session_id;
        let offset = input.offset.unwrap_or(0) as usize;
        // A later page goes on with the answer; the report, often its start, came with the first.
        let reports = if offset == 0 {
            self.host
                .read("agents.result.reports", move |db| store::reports_after(db, child, 0))
                .await?
        } else {
            Vec::new()
        };
        let report = reports
            .iter()
            .rev()
            .find(|stored| stored.report.status.final_report())
            .or(reports.last())
            .cloned()
            .map(|stored| report_view(&member.handle, stored));
        let answer = member.details.last_turn.as_ref().and_then(|turn| turn.answer.clone());
        let answer_chars = answer.as_ref().map_or(0, |text| text.chars().count());
        let limit = input.limit.unwrap_or(ANSWER_PAGE_DEFAULT).clamp(1, ANSWER_PAGE_MAX) as usize;
        let page = answer.map(|text| text.chars().skip(offset).take(limit).collect::<String>());
        let end = offset + limit;
        Ok(AgentsResultResult {
            agent: member.handle.clone(),
            name: member.details.name.clone(),
            state: member.state,
            report,
            answer: page,
            answer_chars: u32::try_from(answer_chars).unwrap_or(u32::MAX),
            next_offset: (end < answer_chars).then(|| u32::try_from(end).unwrap_or(u32::MAX)),
        })
    }

    /// The sub-agents of `parent` as the desktop lists them, after the conversation's own agent.
    pub async fn list(&self, parent: SessionId) -> anyhow::Result<AgentTeamView> {
        let starting = self.host.parent(parent).await?;
        let lead = self.lead(&starting).await?;
        let mut members = Vec::new();
        for member in self.chosen(parent, &[]).await? {
            let child = member.child_session_id;
            let (latest, worktree) = self
                .host
                .read("agents.list.report", move |db| {
                    Ok((store::latest_report(db, child)?, crate::worktree::worktree_of(db, child)?))
                })
                .await?;
            let progress = self.host.progress(child).await?;
            let (summary, question) = latest.as_ref().map_or((None, None), |stored| {
                (Some(cut(&stored.report.summary)), stored.report.question.clone())
            });
            // A question the main agent handed to the user is the user's to decide.
            let reviews = &member.details.reviews;
            let handed = |id: &ApprovalRequestId| {
                reviews.iter().any(|review| review.approval_id == *id && review.answered.is_some())
            };
            let (handed_over, reviewing): (Vec<_>, Vec<_>) = progress
                .reviewing
                .iter()
                .map(|request| request.approval_id)
                .partition(|id| handed(id));
            let mut approvals = progress.approvals;
            approvals.extend(handed_over);
            let answered =
                reviews.iter().filter(|review| review.answered.is_some()).cloned().collect();
            members.push(AgentTeamMemberView {
                agent: member.handle,
                name: member.details.name,
                session_id: child,
                workspace_id: Some(
                    worktree
                        .as_ref()
                        .map_or(starting.workspace_id, |worktree| worktree.workspace_id),
                ),
                model_profile_id: member.details.model_profile_id,
                model: member.details.model_name,
                state: member.state,
                working: progress.working,
                tool: progress.tool,
                approvals,
                reviewing,
                tokens: progress.tokens,
                activity_at: Some(member.updated_at),
                last_report: latest.map(|report| report.report.status),
                summary,
                question,
                worktree: worktree.as_ref().map(|worktree| self.worktree_view(worktree)),
                mode: progress.mode,
                allowed: member.details.allowed,
                reviews: answered,
            });
        }
        Ok(AgentTeamView { lead, members })
    }

    /// The conversation's own agent: its model, and what it does now.
    async fn lead(&self, parent: &crate::Parent) -> anyhow::Result<AgentTeamLeadView> {
        let model_profile_id = parent.model;
        // A model that is no longer enabled keeps its id and loses its name.
        let model = model_profile_id.and_then(|id| {
            self.host
                .models()
                .ok()?
                .into_iter()
                .find(|offer| offer.id == id)
                .map(|offer| offer.name)
        });
        let progress = self.host.progress(parent.session_id).await?;
        Ok(AgentTeamLeadView {
            model_profile_id,
            model,
            working: progress.working,
            tool: progress.tool,
            tokens: progress.tokens,
        })
    }
}

/// A summary as the list carries it: at most [`LIST_SUMMARY_CHARS`], the last one `…` when cut.
fn cut(summary: &str) -> String {
    if summary.chars().count() <= LIST_SUMMARY_CHARS {
        return summary.to_owned();
    }
    let mut cut: String = summary.chars().take(LIST_SUMMARY_CHARS - 1).collect();
    cut.push('…');
    cut
}
