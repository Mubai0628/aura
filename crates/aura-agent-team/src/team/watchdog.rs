//! The watchdog: Core's own rules over every running sub-agent, checked on a clock. Near the token
//! budget the main agent is told once; past it, past the time for one task (the waits for an
//! approval not counted) or long without progress, the sub-agent's turn is stopped and a report
//! says why, so its main agent hears of it and the user sees it among what needs them. What it
//! remembers lives only while Core runs: a restart interrupts every running sub-agent anyway.
use super::*;
use crate::{AgentTeamLimits, Progress, Report, ReportStatus, limits};
use aura_domain_common::TurnTaskId;
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, PoisonError},
    time::Duration,
};

/// With no new answer, report, approval or turn, no model answer under way and no call running, for
/// this long, a sub-agent has made no progress. An answer under way is bounded by its endpoint's own
/// timeouts, as a call is by its own.
pub const NO_PROGRESS: Duration = Duration::from_secs(15 * 60);

/// What the watchdog remembers of each running sub-agent while Core runs.
#[derive(Default)]
pub struct Watchdog {
    watches: Mutex<HashMap<SessionId, Watch>>,
}

/// One running sub-agent as the watchdog saw it last.
struct Watch {
    /// The newest report that ended a piece of its work when this task began: a later one starts
    /// another task.
    task: u32,
    seen: Seen,
    /// Its time on this task so far, the waits for an approval not counted.
    worked: Duration,
    /// How long nothing changed while it neither waited for the user nor ran a call.
    still: Duration,
    /// The token budget the main agent was told it nears.
    warned: Option<u64>,
}

#[derive(Clone, PartialEq, Eq)]
struct Seen {
    turn: Option<TurnTaskId>,
    tokens: u64,
    tool: Option<String>,
    approvals: usize,
    reports: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Fine,
    Warn,
    Stop(Why),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    /// Past this token budget.
    Budget(u64),
    Time,
    Still,
}

impl Watchdog {
    /// What `elapsed` more of `progress` means for sub-agent `child` on task `task`.
    fn observe(
        &self,
        child: SessionId,
        task: u32,
        reports: u32,
        progress: &Progress,
        elapsed: Duration,
        limits: &AgentTeamLimits,
    ) -> Verdict {
        let seen = Seen {
            turn: progress.turn,
            tokens: progress.tokens,
            tool: progress.tool.clone(),
            approvals: progress.approvals.len(),
            reports,
        };
        let mut watches = self.watches.lock().unwrap_or_else(PoisonError::into_inner);
        let watch = watches.entry(child).or_insert_with(|| Watch {
            task,
            seen: seen.clone(),
            worked: Duration::ZERO,
            still: Duration::ZERO,
            warned: None,
        });
        if watch.task != task {
            // New work after a report has a clock of its own.
            watch.task = task;
            watch.worked = Duration::ZERO;
            watch.still = Duration::ZERO;
        }
        let waiting = seen.approvals > 0;
        if !waiting {
            watch.worked += elapsed;
        }
        if waiting || seen.tool.is_some() || progress.answering || watch.seen != seen {
            watch.still = Duration::ZERO;
        } else {
            watch.still += elapsed;
        }
        watch.seen = seen;
        if let Some(budget) = limits.token_budget
            && progress.tokens >= budget
        {
            return Verdict::Stop(Why::Budget(budget));
        }
        if watch.worked >= minutes(limits.minutes_per_task) {
            return Verdict::Stop(Why::Time);
        }
        if watch.still >= NO_PROGRESS {
            return Verdict::Stop(Why::Still);
        }
        if let Some(warning) = limits.warning_tokens()
            && progress.tokens >= warning
            && watch.warned != limits.token_budget
        {
            watch.warned = limits.token_budget;
            return Verdict::Warn;
        }
        Verdict::Fine
    }

    /// Forgets the sub-agents that stopped running.
    fn keep_only(&self, running: &HashSet<SessionId>) {
        let mut watches = self.watches.lock().unwrap_or_else(PoisonError::into_inner);
        watches.retain(|child, _| running.contains(child));
    }
}

impl<H: TeamHost> Team<'_, H> {
    /// One pass over every running sub-agent, `elapsed` after the last one. A sub-agent that
    /// cannot be looked at is left for the next pass; the first such failure is returned.
    pub async fn watch(&self, dog: &Watchdog, elapsed: Duration) -> anyhow::Result<()> {
        let (limits, _) = self.host.read("agents.watch.limits", limits::limits).await?;
        let running = self.host.read("agents.watch.members", store::running_members).await?;
        dog.keep_only(&running.iter().map(|member| member.child_session_id).collect());
        let mut failed = None;
        for member in running {
            if let Err(error) = self.watch_one(dog, &member, elapsed, &limits).await {
                failed.get_or_insert(error);
            }
        }
        failed.map_or(Ok(()), Err)
    }

    async fn watch_one(
        &self,
        dog: &Watchdog,
        member: &Member,
        elapsed: Duration,
        limits: &AgentTeamLimits,
    ) -> anyhow::Result<()> {
        let child = member.child_session_id;
        let progress = self.host.progress(child).await?;
        let (task, reports) = self
            .host
            .read("agents.watch.reports", move |db| {
                let latest = store::latest_report(db, child)?.map_or(0, |report| report.sequence);
                Ok((store::latest_final_sequence(db, child)?, latest))
            })
            .await?;
        match dog.observe(child, task, reports, &progress, elapsed, limits) {
            Verdict::Fine => Ok(()),
            Verdict::Warn => self.warn(member, &progress, limits).await,
            Verdict::Stop(why) => self.stop_by_watchdog(member, why, &progress, limits, task).await,
        }
    }

    /// Tells the main agent its sub-agent nears the token budget; it works on.
    async fn warn(
        &self,
        member: &Member,
        progress: &Progress,
        limits: &AgentTeamLimits,
    ) -> anyhow::Result<()> {
        let child = member.child_session_id;
        let Some(budget) = limits.token_budget else { return Ok(()) };
        let Some(turn) =
            progress.turn.or(member.details.last_turn.as_ref().map(|end| end.turn_task_id))
        else {
            return Ok(());
        };
        let source = format!("watchdog:tokens-warning:{child}:{budget}");
        let report = aura_report(
            ReportStatus::Progress,
            format!(
                "看门狗提醒：它用掉的 token 已超过预算的 {}%（已用 {}，上限 {}）。到上限会被停下；\
                 要它先收尾回答，用 agents.send 告诉它。",
                AgentTeamLimits::WARNING_PERCENT,
                progress.tokens,
                budget
            ),
        );
        let wrote = self
            .host
            .write("agents.watchdog.warn", move |tx| {
                let Some(current) = store::member_by_child(tx, child)? else { return Ok(false) };
                if current.state != MemberState::Running {
                    return Ok(false);
                }
                store::append_report(tx, child, &source, turn, &report, UtcTimestamp::now())?;
                Ok(true)
            })
            .await?;
        if wrote {
            self.news.notify_waiters();
        }
        Ok(())
    }

    /// Marks the sub-agent reported with a failed report saying why, then stops its turn; a
    /// sub-agent that never had a turn has nothing to report from and is stopped.
    async fn stop_by_watchdog(
        &self,
        member: &Member,
        why: Why,
        progress: &Progress,
        limits: &AgentTeamLimits,
        task: u32,
    ) -> anyhow::Result<()> {
        let child = member.child_session_id;
        let turn = progress.turn.or(member.details.last_turn.as_ref().map(|end| end.turn_task_id));
        let (key, summary, note) = match why {
            Why::Budget(budget) => (
                "tokens",
                format!(
                    "看门狗停下了它：token 预算用完了（已用 {}，上限 {budget}）。它没给出最终回答，之前交的报告都在。\
                     预算用完的子 Agent 不再接收消息；还要做的话派一个新的，或请用户在 设置 → 智能能力 → 多 Agent 里调高上限。",
                    progress.tokens
                ),
                super::stop_note::budget_used(progress.tokens, budget),
            ),
            Why::Time => (
                "time",
                format!(
                    "看门狗停下了它：这次干活已超过 {} 分钟（等审批的时间不算），还没给出最终回答。\
                     要它接着干就用 agents.send，不要了就用 agents.stop。",
                    limits.minutes_per_task
                ),
                super::stop_note::over_time(limits.minutes_per_task),
            ),
            Why::Still => (
                "still",
                format!(
                    "看门狗停下了它：{} 分钟没有任何进展（模型没在回答，也没有在跑的调用）。\
                     要它接着干就用 agents.send，不要了就用 agents.stop。",
                    NO_PROGRESS.as_secs() / 60
                ),
                super::stop_note::no_progress(NO_PROGRESS.as_secs() / 60),
            ),
        };
        let source = format!("watchdog:{key}:{child}:{task}");
        let report = aura_report(ReportStatus::Failed, summary);
        let stopped = self
            .host
            .write("agents.watchdog.stop", move |tx| {
                let Some(current) = store::member_by_child(tx, child)? else { return Ok(false) };
                if current.state != MemberState::Running {
                    return Ok(false);
                }
                let at = UtcTimestamp::now();
                let mut next = current.clone();
                match turn {
                    Some(turn) => {
                        store::append_report(tx, child, &source, turn, &report, at)?;
                        next.state = MemberState::Reported;
                    }
                    None => next.state = MemberState::Stopped,
                }
                next.revision += 1;
                next.updated_at = at;
                store::save_member(tx, &current, &next)?;
                Ok(true)
            })
            .await?;
        if stopped {
            self.host.halt(child, Some(note)).await?;
            self.news.notify_waiters();
        }
        Ok(())
    }
}

fn minutes(count: u32) -> Duration {
    Duration::from_secs(u64::from(count) * 60)
}

fn aura_report(status: ReportStatus, summary: String) -> Report {
    Report {
        status,
        summary,
        question: None,
        files: Vec::new(),
        checks: Vec::new(),
        remaining: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(tokens: u64) -> Progress {
        Progress { working: true, turn: Some(TurnTaskId::new()), tokens, ..Progress::default() }
    }

    #[test]
    fn a_budget_stop_comes_before_a_warning_and_a_warning_comes_once_per_budget() {
        let dog = Watchdog::default();
        let child = SessionId::new();
        let limits = AgentTeamLimits { token_budget: Some(100_000), ..AgentTeamLimits::DEFAULT };
        let seen = progress(85_000);
        assert_eq!(dog.observe(child, 0, 0, &seen, Duration::ZERO, &limits), Verdict::Warn);
        assert_eq!(dog.observe(child, 0, 0, &seen, Duration::ZERO, &limits), Verdict::Fine);
        let raised = AgentTeamLimits { token_budget: Some(105_000), ..limits };
        assert_eq!(dog.observe(child, 0, 0, &seen, Duration::ZERO, &raised), Verdict::Warn);
        let spent = progress(105_000);
        assert_eq!(
            dog.observe(child, 0, 0, &spent, Duration::ZERO, &raised),
            Verdict::Stop(Why::Budget(105_000))
        );
    }

    #[test]
    fn without_a_budget_no_count_of_tokens_warns_or_stops() {
        let dog = Watchdog::default();
        let child = SessionId::new();
        let seen = progress(500_000_000);
        let limits = AgentTeamLimits::DEFAULT;
        assert_eq!(dog.observe(child, 0, 0, &seen, Duration::ZERO, &limits), Verdict::Fine);
    }

    #[test]
    fn an_answer_under_way_is_progress_however_long_it_streams() {
        let dog = Watchdog::default();
        let child = SessionId::new();
        let limits = AgentTeamLimits::DEFAULT;
        let answering = Progress { answering: true, ..progress(10) };
        for _ in 0..6 {
            let verdict =
                dog.observe(child, 0, 0, &answering, Duration::from_secs(10 * 60), &limits);
            assert_eq!(verdict, Verdict::Fine);
        }
        // The answer ended and nothing came after it: the quiet time counts from there.
        let quiet = Progress { answering: false, ..answering.clone() };
        assert_eq!(dog.observe(child, 0, 0, &quiet, NO_PROGRESS / 2, &limits), Verdict::Fine);
        assert_eq!(
            dog.observe(child, 0, 0, &quiet, NO_PROGRESS / 2, &limits),
            Verdict::Stop(Why::Still)
        );
    }
}
