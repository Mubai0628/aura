//! The tools' behaviour over a [`TeamHost`].
mod control;
mod desktop;
mod failure;
mod follow;
mod reports;
mod review;
mod spawn;
mod stop_note;
mod watchdog;
mod worktree;

use crate::{
    DISCARD, MERGE, Member, MemberState, Refusal, ReportView, TeamCall, TeamHost, store,
    store::StoredReport, tools::*,
};
use aura_domain_common::{CommandInvocationId, SessionId, ToolCallId, UtcTimestamp};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use tokio::sync::Notify;

pub use failure::{cause_of, main_agent_denial, one_line};
pub use follow::{WAIT_DEFAULT_SECONDS, WAIT_MAX_SECONDS};
pub(crate) use reports::from_turn_end;
pub use spawn::choose_model;
pub use watchdog::Watchdog;

pub struct Team<'h, H: TeamHost> {
    host: &'h H,
    news: Arc<Notify>,
}

impl<'h, H: TeamHost> Team<'h, H> {
    /// `news` is the port's: woken by every report and every ended turn of a sub-agent.
    pub fn new(host: &'h H, news: Arc<Notify>) -> Self {
        Self { host, news }
    }

    /// Runs one call; the error is the text the model is shown: a refusal or a step's own words as
    /// they are, any other failure with what the call was doing and what it may have changed.
    pub async fn call(&self, call: TeamCall) -> Result<serde_json::Value, String> {
        let outcome = match call.tool.as_str() {
            SPAWN => self.spawn(&call).await.and_then(value),
            STATUS => self.status(&call).await.and_then(value),
            WAIT => self.wait(&call).await.and_then(value),
            SEND => self.send(&call).await.and_then(value),
            RESULT => self.result(&call).await.and_then(value),
            STOP => self.stop(&call).await.and_then(value),
            REPORT => self.report(&call).await.and_then(value),
            MERGE => self.merge(&call).await.and_then(value),
            DISCARD => self.discard(&call).await.and_then(value),
            crate::APPROVE => self.approve(&call).await.and_then(value),
            other => Err(Refusal::new("agents.unknown_tool", format!("没有 {other}。")).into()),
        };
        outcome.map_err(|error| failure::explain(&call.tool, &error))
    }

    /// The members of `parent` the call names: one by handle, or all of them.
    async fn chosen(&self, parent: SessionId, handles: &[String]) -> anyhow::Result<Vec<Member>> {
        let members =
            self.host.read("agents.members", move |db| store::members_of(db, parent)).await?;
        if handles.is_empty() {
            return Ok(members);
        }
        handles
            .iter()
            .map(|handle| {
                members
                    .iter()
                    .find(|member| member.handle.eq_ignore_ascii_case(handle.trim()))
                    .cloned()
                    .ok_or_else(|| unknown_agent(handle, &members).into())
            })
            .collect()
    }

    async fn one(&self, parent: SessionId, handle: &str) -> anyhow::Result<Member> {
        let mut chosen = self.chosen(parent, &[handle.to_owned()]).await?;
        chosen.pop().ok_or_else(|| unknown_agent(handle, &[]).into())
    }

    /// Refuses a sub-agent's call of a main agent's tool.
    async fn refuse_subagent(&self, session: SessionId) -> anyhow::Result<()> {
        let member =
            self.host.read("agents.caller", move |db| store::member_by_child(db, session)).await?;
        if member.is_some() {
            return Err(Refusal::new(
                "agents.not_for_subagents",
                "子 Agent 不能派或管理子 Agent。",
            )
            .into());
        }
        Ok(())
    }

    /// Writes `member` with its state and details changed by `change`, one revision on.
    async fn change(
        &self,
        member: &Member,
        change: impl FnOnce(&mut Member) + Send + 'static,
    ) -> anyhow::Result<Member> {
        let child = member.child_session_id;
        self.host
            .write("agents.member.save", move |tx| {
                let current = store::member_by_child(tx, child)?
                    .ok_or(aura_persistence_core::PersistenceCoreError::Conflict)?;
                let mut next = current.clone();
                change(&mut next);
                next.revision = current.revision + 1;
                next.updated_at = UtcTimestamp::now();
                store::save_member(tx, &current, &next)?;
                Ok(next)
            })
            .await
    }

    /// Refuses one more sub-agent of `parent` at work when the user's limit is reached. One
    /// waiting for an approval holds no slot.
    async fn take_slot(&self, parent: SessionId) -> anyhow::Result<()> {
        let (limits, _) = self.host.read("agents.limits", crate::limits::limits).await?;
        let members =
            self.host.read("agents.slots", move |db| store::members_of(db, parent)).await?;
        let mut working = 0_u32;
        for member in members.iter().filter(|member| member.state == MemberState::Running) {
            if self.host.progress(member.child_session_id).await?.approvals.is_empty() {
                working += 1;
            }
        }
        if working >= limits.max_running {
            return Err(no_slot(limits.max_running).into());
        }
        Ok(())
    }
}

fn value<T: Serialize>(result: T) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::to_value(result)?)
}

fn parse<T: DeserializeOwned>(arguments: &serde_json::Value) -> anyhow::Result<T> {
    serde_json::from_value(arguments.clone()).map_err(|error| {
        Refusal::new("agents.input_invalid", failure::input_problem(&error)).into()
    })
}

fn unknown_agent(handle: &str, members: &[Member]) -> Refusal {
    let known: Vec<&str> = members.iter().map(|member| member.handle.as_str()).collect();
    let message = if known.is_empty() {
        format!("没有子 Agent {handle}。")
    } else {
        format!("没有子 Agent {handle}。这个会话的子 Agent：{}。", known.join("、"))
    };
    Refusal::new("agents.unknown_agent", message)
}

fn no_slot(limit: u32) -> Refusal {
    Refusal::new(
        "agents.no_slot",
        format!(
            "同时最多 {limit} 个子 Agent 在干活（等审批的不算）。先用 agents.wait 等它们交报告，或用 agents.stop 停掉一个。"
        ),
    )
}

fn report_view(handle: &str, stored: StoredReport) -> ReportView {
    ReportView {
        agent: handle.to_owned(),
        sequence: stored.sequence,
        report: stored.report,
        by_turn_end: stored.by_turn_end,
        at: stored.created_at.to_string(),
    }
}

/// The command identity of one step a team call takes, the same each time the call is made.
fn command_for(call: ToolCallId, step: &str) -> CommandInvocationId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aura.agent-team.command.v1");
    hasher.update(&call.to_bytes());
    hasher.update(step.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    CommandInvocationId::from_bytes(bytes)
}
