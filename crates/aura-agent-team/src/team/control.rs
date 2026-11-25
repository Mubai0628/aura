//! Steering the sub-agents: `agents.send` and `agents.stop`, and stopping them all when the turn
//! that started them is stopped.
use super::*;
use crate::StopNote;
use crate::{Report, ReportStatus, session_reference};

const SEND_MAX_CHARS: usize = 8_000;

impl<H: TeamHost> Team<'_, H> {
    pub(super) async fn send(&self, call: &TeamCall) -> anyhow::Result<AgentsSendResult> {
        let input: AgentsSendInput = parse(&call.arguments)?;
        self.refuse_subagent(call.session_id).await?;
        self.allow_new_work(call.session_id).await?;
        let text = input.text.trim();
        if text.is_empty() || text.chars().count() > SEND_MAX_CHARS {
            return Err(Refusal::new("agents.send_invalid", "text 不能为空，最多 8000 字。").into());
        }
        if let Some(reference) = session_reference(text) {
            return Err(Refusal::new(
                "agents.send_names_conversation",
                format!("消息里有会话引用 {reference}：子 Agent 不能读别的会话，把需要的事实直接写出来。"),
            )
            .into());
        }
        let member = self.one(call.session_id, &input.agent).await?;
        let child = member.child_session_id;
        if let Some(worktree) = self
            .host
            .read("agents.send.worktree", move |db| crate::worktree::worktree_of(db, child))
            .await?
            && worktree.state != crate::WorktreeState::Active
        {
            return Err(super::worktree::closed(&worktree).into());
        }
        let reopened = match member.state {
            MemberState::Stopped => {
                return Err(Refusal::new(
                    "agents.stopped",
                    format!("子 Agent {} 已停止，不能再发消息。", member.handle),
                )
                .into());
            }
            MemberState::Running => None,
            MemberState::Reported => {
                self.refuse_used_up(&member).await?;
                // It goes back to work, so it takes a slot again.
                self.take_slot(call.session_id).await?;
                Some(self.change(&member, |next| next.state = MemberState::Running).await?)
            }
        };
        let message = format!("来自主 Agent：{text}");
        match self
            .host
            .deliver(member.child_session_id, message, command_for(call.tool_call_id, "send"))
            .await
        {
            Ok(delivered) => Ok(AgentsSendResult {
                agent: member.handle,
                delivered,
                state: MemberState::Running,
            }),
            Err(error) => {
                if let Some(reopened) = reopened {
                    let _ = self.change(&reopened, |next| next.state = MemberState::Reported).await;
                }
                Err(anyhow::anyhow!(super::failure::not_delivered(&member.handle, &error)))
            }
        }
    }

    /// Refuses more work for a sub-agent whose answers used up the token budget.
    async fn refuse_used_up(&self, member: &Member) -> anyhow::Result<()> {
        let (limits, _) = self.host.read("agents.limits", crate::limits::limits).await?;
        let Some(budget) = limits.token_budget else { return Ok(()) };
        let used = self.host.progress(member.child_session_id).await?.tokens;
        if used >= budget {
            return Err(Refusal::new(
                "agents.budget_used_up",
                format!(
                    "子 Agent {} 的 token 预算用完了（已用 {used}，上限 {budget}），不再接收消息。还要做的话派一个新的，或请用户在 设置 → 智能能力 → 多 Agent 里调高上限。",
                    member.handle
                ),
            )
            .into());
        }
        Ok(())
    }

    pub(super) async fn stop(&self, call: &TeamCall) -> anyhow::Result<AgentsStopResult> {
        let input: AgentsStopInput = parse(&call.arguments)?;
        self.refuse_subagent(call.session_id).await?;
        let member = self.one(call.session_id, &input.agent).await?;
        let (stopped, halted) =
            self.mark_stopped(member, Some(super::stop_note::by_main_agent())).await?;
        if let Err(error) = halted {
            return Err(anyhow::anyhow!(super::failure::still_ending(&stopped.handle, &error)));
        }
        Ok(AgentsStopResult { agent: stopped.handle, state: stopped.state })
    }

    /// The user stops sub-agent `handle` of `parent` from the 多 Agent page. It stops as with
    /// `agents.stop`, and the main agent is told in a report of its own, so a wait for the
    /// sub-agent ends saying why. Stopping a stopped one changes nothing.
    pub async fn stop_for_user(&self, parent: SessionId, handle: &str) -> anyhow::Result<()> {
        let member = self.one(parent, handle).await?;
        if member.state == MemberState::Stopped {
            return Ok(());
        }
        let child = member.child_session_id;
        // The report belongs to the turn it stopped, or to the last one it worked in.
        let turn = self.host.progress(child).await?.turn.or(member
            .details
            .last_turn
            .as_ref()
            .map(|end| end.turn_task_id));
        let stopped = self
            .host
            .write("agents.user_stop", move |tx| {
                let Some(current) = store::member_by_child(tx, child)? else { return Ok(false) };
                if current.state == MemberState::Stopped {
                    return Ok(false);
                }
                if let Some(turn) = turn {
                    let source = store::user_stop_source(child);
                    store::append_report(
                        tx,
                        child,
                        &source,
                        turn,
                        &stopped_by_user(),
                        UtcTimestamp::now(),
                    )?;
                }
                let mut next = current.clone();
                next.state = MemberState::Stopped;
                next.revision += 1;
                next.updated_at = UtcTimestamp::now();
                store::save_member(tx, &current, &next)?;
                Ok(true)
            })
            .await?;
        if stopped {
            self.host.halt(child, None).await?;
            self.news.notify_waiters();
        }
        Ok(())
    }

    /// Stops the sub-agents of `parent` that are running: the user stopped the turn that
    /// started them.
    pub async fn stop_running(&self, parent: SessionId) -> anyhow::Result<()> {
        for member in self.chosen(parent, &[]).await? {
            if member.state == MemberState::Running {
                self.halt(member, super::stop_note::with_main_turn()).await?;
            }
        }
        Ok(())
    }

    /// Marks the member stopped first, so the end of its turn is not taken for a report, then
    /// stops the turn. An unknown result of a call it was making stays unknown.
    pub(super) async fn halt(&self, member: Member, note: StopNote) -> anyhow::Result<Member> {
        let (stopped, halted) = self.mark_stopped(member, Some(note)).await?;
        halted.map(|()| stopped)
    }

    /// As [`Self::halt`], with how stopping the turn ended apart from the member as marked: one
    /// whose turn did not stop in time is marked stopped all the same. `note` says who stopped it
    /// and why, for its next turn to read.
    pub(super) async fn mark_stopped(
        &self,
        member: Member,
        note: Option<StopNote>,
    ) -> anyhow::Result<(Member, anyhow::Result<()>)> {
        if member.state == MemberState::Stopped {
            return Ok((member, Ok(())));
        }
        let stopped = self.change(&member, |next| next.state = MemberState::Stopped).await?;
        let halted = self.host.halt(stopped.child_session_id, note).await;
        if halted.is_ok() {
            self.news.notify_waiters();
        }
        Ok((stopped, halted))
    }
}

/// The report the main agent is given when the user stops its sub-agent.
fn stopped_by_user() -> Report {
    Report {
        status: ReportStatus::Failed,
        summary: "用户在多 Agent 页停止了这个子 Agent：它不再干活，也不再接收消息。".to_owned(),
        question: None,
        files: Vec::new(),
        checks: Vec::new(),
        remaining: None,
    }
}
