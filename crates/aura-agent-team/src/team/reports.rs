//! A sub-agent's reports: `agent.report`, which tells progress or asks the main agent a question,
//! and the report each of its turns ends with: its final answer, or why there is none.
use super::*;
use crate::{Report, ReportInput, ReportStatus, TurnEnd};
use rusqlite::OptionalExtension;

const SUMMARY_FROM_ANSWER_CHARS: usize = 8_000;

/// What a sub-agent says in its final answer, not with `agent.report`.
const FINAL_STATUSES: [&str; 3] = ["ready", "blocked", "failed"];

enum Taken {
    Accepted { sequence: u32 },
    Closed(MemberState),
}

impl<H: TeamHost> Team<'_, H> {
    pub(super) async fn report(&self, call: &TeamCall) -> anyhow::Result<AgentReportResult> {
        let status = call.arguments.get("status").and_then(serde_json::Value::as_str);
        if status.is_some_and(|status| FINAL_STATUSES.contains(&status)) {
            return Err(Refusal::new(
                "agent.report_final_answer",
                "做完、受阻或做不成都不用 agent.report：直接用这一轮的最终回答交代结果（先写结果，再写依据的文件、\
                 跑过的检查和退出码、没做完的），主 Agent 会收到它。",
            )
            .into());
        }
        let report = parse::<ReportInput>(&call.arguments)?.into_report();
        let child = call.session_id;
        let member = self
            .host
            .read("agent.report.member", move |db| store::member_by_child(db, child))
            .await?;
        if member.is_none() {
            return Err(Refusal::new(
                "agent.report_not_subagent",
                "只有子 Agent 交报告；这个会话直接回复用户。",
            )
            .into());
        }
        report.check()?;
        let source = call.tool_call_id.to_string();
        let turn = call.turn_task_id;
        let status = report.status;
        let taken = self
            .host
            .write("agent.report", move |tx| {
                let Some(current) = store::member_by_child(tx, child)? else {
                    return Ok(Taken::Closed(MemberState::Stopped));
                };
                let repeated = tx
                    .query_row(
                        "SELECT sequence FROM agent_team_reports WHERE source=?1",
                        [&source],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?;
                if repeated.is_none() && current.state != MemberState::Running {
                    return Ok(Taken::Closed(current.state));
                }
                let sequence =
                    store::append_report(tx, child, &source, turn, &report, UtcTimestamp::now())?;
                if repeated.is_none() && report.status.final_report() {
                    let mut next = current.clone();
                    next.state = MemberState::Reported;
                    next.revision += 1;
                    next.updated_at = UtcTimestamp::now();
                    store::save_member(tx, &current, &next)?;
                }
                Ok(Taken::Accepted { sequence })
            })
            .await?;
        let sequence = match taken {
            Taken::Accepted { sequence } => sequence,
            Taken::Closed(MemberState::Stopped) => {
                return Err(Refusal::new("agent.report_stopped", "这个子 Agent 已被停止。").into());
            }
            Taken::Closed(_) => {
                return Err(Refusal::new(
                    "agent.report_closed",
                    "已经向主 Agent 提了问题。用一两句话结束这一轮，等它回复。",
                )
                .into());
            }
        };
        self.news.notify_waiters();
        let next = if status.final_report() {
            "问题已交给主 Agent。现在用一两句话结束这一轮，不再调用工具；它的回复会作为新消息发来。"
        } else {
            "进展已交给主 Agent，接着干。"
        };
        Ok(AgentReportResult { sequence, next: next.to_owned() })
    }

    /// Records how a turn of conversation `child` ended. A sub-agent still running, one that did
    /// not stop for a question, reports with the turn's end: its final answer, or why it has none,
    /// `cause` in words when the caller knows them.
    pub async fn turn_ended(
        &self,
        child: SessionId,
        end: TurnEnd,
        cause: Option<String>,
    ) -> anyhow::Result<()> {
        let wrote = self
            .host
            .write("agents.turn_ended", move |tx| {
                let source = store::turn_end_source(end.turn_task_id);
                store::record_turn_end(
                    tx,
                    child,
                    &end,
                    &from_turn_end(&end, cause.as_deref()),
                    &source,
                    UtcTimestamp::now(),
                )
            })
            .await?;
        if wrote {
            self.news.notify_waiters();
        }
        Ok(())
    }
}

/// What the main agent may do with a sub-agent whose turn stopped before it finished.
const AFTER_STOP: &str = "要它接着干就用 agents.send，不要了就用 agents.stop。";

/// The report a turn ends with; `cause` says in words why a turn that did not complete stopped.
pub(crate) fn from_turn_end(end: &TurnEnd, cause: Option<&str>) -> Report {
    let (status, summary) = match end.outcome.as_str() {
        "completed" => (
            ReportStatus::Ready,
            end.answer
                .as_deref()
                .filter(|answer| !answer.trim().is_empty())
                .map_or_else(|| "这一轮结束了，没有回答。".to_owned(), answer_start),
        ),
        "cancelled" => (ReportStatus::Failed, format!("这一轮被停止了，没有回答。{AFTER_STOP}")),
        _ => (
            ReportStatus::Failed,
            match cause {
                Some(cause) => format!("这一轮没做完就停了：{cause}。{AFTER_STOP}"),
                None => format!(
                    "这一轮没做完就停了，没有回答（{}）。{AFTER_STOP}",
                    end.reason.as_deref().unwrap_or("原因没有记下")
                ),
            }
            .chars()
            .take(SUMMARY_FROM_ANSWER_CHARS)
            .collect(),
        ),
    };
    Report { status, summary, question: None, files: vec![], checks: vec![], remaining: None }
}

/// A final answer as the main agent is given it: whole, or its start and its length, counted as
/// `agents.result` pages it.
fn answer_start(answer: &str) -> String {
    let total = answer.chars().count();
    let mut start: String = answer.chars().take(SUMMARY_FROM_ANSWER_CHARS).collect();
    if total > SUMMARY_FROM_ANSWER_CHARS {
        start.push_str(&format!(
            "\n\n（回答共 {total} 字，这里是前 {SUMMARY_FROM_ANSWER_CHARS} 字；后面的用 agents.result 读，offset {SUMMARY_FROM_ANSWER_CHARS}。）"
        ));
    }
    start
}
