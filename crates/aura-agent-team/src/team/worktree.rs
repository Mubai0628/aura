//! Worktrees in the team: adding one for a sub-agent, merging and discarding one, what one
//! changed, the files two of them changed, and their removal with the conversation.
use super::*;
use crate::{
    AgentTeamChangesView, AgentWorktreeView, AgentsDiscardInput, AgentsDiscardResult,
    AgentsMergeInput, AgentsMergeResult, Worktree, WorktreeDetails, WorktreeMerge, WorktreeState,
    worktree as rows,
};
use aura_domain_common::WorkspaceId;
use std::collections::BTreeMap;

/// The most files changed by two or more sub-agents the main agent is told of at once.
const OVERLAPS_MAX: usize = 20;

/// The worktree a call names, with the sub-agent working in it when it is a sub-agent's.
struct Target {
    worktree: Worktree,
    member: Option<Member>,
}

/// A worktree added for a spawn call, and how many of the workspace's uncommitted files it took
/// along.
pub(super) struct Placed {
    pub worktree: Worktree,
    pub carried: usize,
}

impl<H: TeamHost> Team<'_, H> {
    /// Adds a worktree of `workspace` for the spawn call. Its folder and branch are named after
    /// the call, so the same call made again finds the worktree it added.
    pub(super) async fn place_worktree(
        &self,
        workspace: WorkspaceId,
        call: ToolCallId,
    ) -> anyhow::Result<Placed> {
        let key = worktree_key(&call.to_bytes());
        self.add(workspace, &key, command_for(call, "worktree")).await
    }

    async fn add(
        &self,
        workspace: WorkspaceId,
        key: &str,
        command: CommandInvocationId,
    ) -> anyhow::Result<Placed> {
        let plan = self.host.plan_worktree(workspace, key).await?;
        let added = self.host.add_worktree(workspace, &plan, command).await?;
        let now = UtcTimestamp::now();
        let worktree = Worktree {
            workspace_id: added.workspace_id,
            // Named once the conversation working in it is open.
            session_id: SessionId::new(),
            parent_session_id: None,
            parent_workspace_id: workspace,
            state: WorktreeState::Active,
            revision: 1,
            details: WorktreeDetails { path: plan.path, branch: plan.branch, base: added.base },
            created_at: now,
            updated_at: now,
        };
        Ok(Placed { worktree, carried: added.carried.len() })
    }

    /// Removes a worktree added for a conversation that never started in it.
    pub(super) async fn take_back(&self, worktree: &Worktree, call: ToolCallId) {
        let _ = self.host.remove_worktree(worktree, command_for(call, "take_back")).await;
    }

    pub(super) async fn merge(&self, call: &TeamCall) -> anyhow::Result<AgentsMergeResult> {
        let input: AgentsMergeInput = parse(&call.arguments)?;
        let Target { worktree, member } =
            self.target(call.session_id, input.agent.as_deref()).await?;
        if let Some(member) = &member
            && member.state == MemberState::Running
        {
            return Err(Refusal::new(
                "agents.still_running",
                format!(
                    "子 Agent {} 还在干活：等它交报告（agents.wait）或先停下它，再合回。",
                    member.handle
                ),
            )
            .into());
        }
        let agent = member.as_ref().map(|member| member.handle.clone());
        let message = match &member {
            Some(member) => format!("合回子 Agent {}：{}", member.handle, member.details.name),
            None => format!("合回工作副本 {}", worktree.details.branch),
        };
        let merged = match self
            .host
            .merge_worktree(&worktree, &message, command_for(call.tool_call_id, "merge"))
            .await
        {
            Ok(merged) => merged,
            Err(error) => return Err(anyhow::anyhow!(super::failure::merge_failed(&error))),
        };
        let (head, files) = match merged {
            WorktreeMerge::Conflicted { files } => {
                let note = match &member {
                    Some(member) => format!(
                        "有冲突，什么都没合回，主工作区保持原样。可以让 {} 在它的工作副本里先合并主工作区的分支、解决冲突后再交报告，或者请用户处理。",
                        member.handle
                    ),
                    None => "有冲突，什么都没合回，主工作区保持原样。可以在这个工作副本里先合并主工作区的分支、解决冲突后再合回，或者请用户处理。".to_owned(),
                };
                return Ok(AgentsMergeResult {
                    agent,
                    merged: false,
                    head: None,
                    files: Vec::new(),
                    conflicts: files,
                    note,
                });
            }
            WorktreeMerge::Clashed { files } => {
                let whose = member
                    .as_ref()
                    .map_or("这个工作副本".to_owned(), |member| member.handle.clone());
                return Ok(AgentsMergeResult {
                    agent,
                    merged: false,
                    head: None,
                    files: Vec::new(),
                    conflicts: files,
                    note: format!(
                        "主工作区这些文件里没提交的改动和 {whose} 的改动冲突，什么都没写，主工作区保持原样，工作副本也还在。请用户先提交或处理这些文件里的改动，再合回。"
                    ),
                });
            }
            WorktreeMerge::UpToDate => (None, Vec::new()),
            WorktreeMerge::Merged { head, files } => (Some(head), files),
            WorktreeMerge::Applied { files } => (None, files),
        };
        let mut note = match (&head, files.is_empty()) {
            (Some(_), _) => "已合回主工作区的当前分支。".to_owned(),
            (None, false) => "已把改动写进主工作区的文件，没有提交：主工作区有没提交的改动，或工作副本建时带上了它们。用户原有的改动都还在，提交由用户决定。".to_owned(),
            (None, true) => "没有新的改动要合回。".to_owned(),
        };
        if member.is_some() {
            // A sub-agent's worktree has served its task: it goes now, with its branch.
            match self.close(&worktree, WorktreeState::Merged, call.tool_call_id).await {
                Ok(()) => note.push_str("工作副本和它的分支已删除。"),
                Err(error) => note.push_str(&format!(
                    "工作副本没删掉：{}。处理后再调一次 agents.merge 会再删。",
                    super::failure::cause_of(&error)
                )),
            }
            self.news.notify_waiters();
        } else {
            note.push_str("这个会话继续在工作副本里工作。");
        }
        Ok(AgentsMergeResult { agent, merged: true, head, files, conflicts: Vec::new(), note })
    }

    pub(super) async fn discard(&self, call: &TeamCall) -> anyhow::Result<AgentsDiscardResult> {
        let input: AgentsDiscardInput = parse(&call.arguments)?;
        let Target { worktree, member } =
            self.target(call.session_id, input.agent.as_deref()).await?;
        // It cannot go on without its worktree.
        if let Some(member) = member.clone() {
            let (stopped, halted) =
                self.mark_stopped(member, Some(super::stop_note::worktree_discarded())).await?;
            if let Err(error) = halted {
                let said = super::failure::discard_not_halted(&stopped.handle, &error);
                return Err(anyhow::anyhow!(said));
            }
        }
        if let Err(error) = self.close(&worktree, WorktreeState::Discarded, call.tool_call_id).await
        {
            let said = super::failure::not_removed(member.as_ref(), &worktree, &error);
            return Err(anyhow::anyhow!(said));
        }
        self.news.notify_waiters();
        let note = match &member {
            Some(member) => {
                format!("子 Agent {} 已停止，它的工作副本和分支已删除。", member.handle)
            }
            None => "工作副本和它的分支已删除；这个会话不能再改文件了。".to_owned(),
        };
        Ok(AgentsDiscardResult { agent: member.map(|member| member.handle), discarded: true, note })
    }

    /// What a sub-agent's worktree changed, or the conversation's own when `agent` is none, with
    /// the checks of its newest final report.
    pub async fn changes(
        &self,
        session: SessionId,
        agent: Option<&str>,
    ) -> anyhow::Result<AgentTeamChangesView> {
        let Target { worktree, member } = self.target(session, agent).await?;
        let files = self.host.worktree_changes(&worktree).await?;
        let (checks, remaining) = match &member {
            Some(member) => {
                let child = member.child_session_id;
                self.host
                    .read("agents.changes.report", move |db| store::reports_after(db, child, 0))
                    .await?
                    .into_iter()
                    .rev()
                    .find(|stored| stored.report.status.final_report())
                    .map_or((Vec::new(), None), |stored| {
                        (stored.report.checks, stored.report.remaining)
                    })
            }
            None => (Vec::new(), None),
        };
        Ok(AgentTeamChangesView {
            agent: member.as_ref().map(|member| member.handle.clone()),
            name: member.as_ref().map(|member| member.details.name.clone()),
            member_state: member.as_ref().map(|member| member.state),
            branch: worktree.details.branch.clone(),
            state: worktree.state,
            files,
            checks,
            remaining,
            size_bytes: self.host.worktree_size(&worktree),
        })
    }

    /// Files two or more of `parent`'s sub-agents changed in their worktrees, for the main agent
    /// to see before it merges them. A worktree whose changes cannot be read now is left out; the
    /// merge still finds every conflict.
    pub(super) async fn overlaps(&self, parent: SessionId) -> anyhow::Result<Vec<String>> {
        let (worktrees, members) = self
            .host
            .read("agents.overlaps", move |db| {
                Ok((rows::worktrees_of_parent(db, parent)?, store::members_of(db, parent)?))
            })
            .await?;
        let mut touched: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for worktree in worktrees.iter().filter(|worktree| worktree.state == WorktreeState::Active)
        {
            let Some(member) =
                members.iter().find(|member| member.child_session_id == worktree.session_id)
            else {
                continue;
            };
            let Ok(changes) = self.host.worktree_changes(worktree).await else { continue };
            for change in changes {
                touched.entry(change.path).or_default().push(member.handle.clone());
            }
        }
        Ok(touched
            .into_iter()
            .filter(|(_, handles)| handles.len() > 1)
            .take(OVERLAPS_MAX)
            .map(|(path, mut handles)| {
                handles.sort_by(|left, right| (left.len(), left).cmp(&(right.len(), right)));
                format!("{} 都改了 {path}，合回时可能冲突", handles.join("、"))
            })
            .collect())
    }

    /// Removes the worktrees conversation `session` works in or its sub-agents work in, before
    /// the conversation's records go. One that cannot be removed now is an error, so the
    /// deletion is finished later rather than leaving its folder behind.
    pub async fn remove_session_worktrees(&self, session: SessionId) -> anyhow::Result<()> {
        let worktrees = self
            .host
            .read("agents.worktrees.session", move |db| {
                let mut worktrees = rows::worktrees_of_parent(db, session)?;
                worktrees.extend(rows::worktree_of(db, session)?);
                Ok(worktrees)
            })
            .await?;
        let mut failed = None;
        for worktree in worktrees.iter().filter(|worktree| worktree.state == WorktreeState::Active)
        {
            let command = CommandInvocationId::new();
            let removed = self.host.remove_worktree(worktree, command).await;
            let closed = match removed {
                Ok(()) => self.mark(worktree, WorktreeState::Discarded).await,
                Err(error) => Err(error),
            };
            if let Err(error) = closed {
                failed.get_or_insert(error);
            }
        }
        failed.map_or(Ok(()), Err)
    }

    pub(super) fn worktree_view(&self, worktree: &Worktree) -> AgentWorktreeView {
        AgentWorktreeView {
            branch: worktree.details.branch.clone(),
            state: worktree.state,
            size_bytes: self.host.worktree_size(worktree),
        }
    }

    /// The worktree a sub-agent of `session` works in, or the one `session` itself works in when
    /// `agent` is none. Only a worktree still in use is named; a sub-agent never names one.
    async fn target(&self, session: SessionId, agent: Option<&str>) -> anyhow::Result<Target> {
        self.refuse_subagent(session).await?;
        let member = match agent.map(str::trim).filter(|agent| !agent.is_empty()) {
            Some(handle) => Some(self.one(session, handle).await?),
            None => None,
        };
        let owner = member.as_ref().map_or(session, |member| member.child_session_id);
        let worktree =
            self.host.read("agents.worktree", move |db| rows::worktree_of(db, owner)).await?;
        let Some(worktree) = worktree else {
            let message = match &member {
                Some(member) => {
                    format!("子 Agent {} 没有工作副本：它是只读的，没有改文件。", member.handle)
                }
                None => "这个会话不在工作副本里。要合回或丢弃子 Agent 的工作副本，写上 agent。"
                    .to_owned(),
            };
            return Err(Refusal::new("agents.no_worktree", message).into());
        };
        if worktree.state != WorktreeState::Active {
            return Err(closed(&worktree).into());
        }
        Ok(Target { worktree, member })
    }

    /// Removes the worktree's folder and branch, then records it closed. A removal that fails
    /// leaves it in use, so the same call made again tries once more.
    async fn close(
        &self,
        worktree: &Worktree,
        state: WorktreeState,
        call: ToolCallId,
    ) -> anyhow::Result<()> {
        self.host.remove_worktree(worktree, command_for(call, "remove")).await?;
        self.mark(worktree, state).await
    }

    async fn mark(&self, worktree: &Worktree, state: WorktreeState) -> anyhow::Result<()> {
        let workspace = worktree.workspace_id;
        self.host
            .write("agents.worktree.close", move |tx| {
                let current = rows::worktree_by_workspace(tx, workspace)?
                    .ok_or(aura_persistence_core::PersistenceCoreError::Conflict)?;
                if current.state != WorktreeState::Active {
                    return Ok(());
                }
                rows::set_state(tx, &current, state).map(drop)
            })
            .await
    }
}

/// The refusal of a worktree that was merged or discarded.
pub(super) fn closed(worktree: &Worktree) -> Refusal {
    let done = if worktree.state == WorktreeState::Merged { "合回" } else { "丢弃" };
    Refusal::new(
        "agents.worktree_closed",
        format!(
            "工作副本 {} 已经{done}了，里面不能再改，也没有要合回的。",
            worktree.details.branch
        ),
    )
}

/// The eight hex digits a worktree's folder and branch are named with.
fn worktree_key(seed: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aura.agent-team.worktree.v1");
    hasher.update(seed);
    hasher.finalize().as_bytes()[..4].iter().map(|byte| format!("{byte:02x}")).collect()
}
