//! A host whose store is a real execution store and whose conversations are recorded calls.
#![allow(dead_code, clippy::unwrap_used)]

use aura_agent_team::{
    AddedWorktree, ApprovalAnswer, Delivery, MergeStep, ModelOffer, Parent, Progress, StopNote,
    Team, TeamCall, TeamHost, TurnEnd, Worktree, WorktreeChange, WorktreeHost, WorktreeMerge,
    WorktreePlan,
};
use aura_domain_common::{
    ApprovalRequestId, CommandInvocationId, ModelProfileId, SessionId, ToolCallId, TurnTaskId,
    WorkspaceId,
};
use aura_persistence_core::StoreResult;
use aura_persistence_v2::V2Store;
use rusqlite::{Connection, Transaction};
use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;

pub struct FakeHost {
    store: Mutex<V2Store>,
    pub models: Vec<ModelOffer>,
    pub parent_model: Option<ModelProfileId>,
    pub workspace: WorkspaceId,
    pub opened: Mutex<Vec<(SessionId, String, ModelProfileId)>>,
    pub discarded: Mutex<Vec<SessionId>>,
    pub delivered: Mutex<Vec<(SessionId, String, CommandInvocationId)>>,
    pub halted: Mutex<Vec<SessionId>>,
    /// The note each halt left for the sub-agent's next turn, in order.
    pub stop_notes: Mutex<Vec<(SessionId, Option<StopNote>)>>,
    pub working: Mutex<BTreeSet<SessionId>>,
    /// The turn each working conversation is on.
    pub turns: Mutex<HashMap<SessionId, TurnTaskId>>,
    /// What a conversation shows, when a test sets it; otherwise whether it works and its turn.
    pub shown: Mutex<HashMap<SessionId, Progress>>,
    pub fail_delivery: AtomicBool,
    /// Why deliveries fail when they do; `agent.model_binding_required` unless a test says.
    pub delivery_failure: Mutex<Option<String>>,
    pub news: Arc<Notify>,
    /// Whether the parent's workspace is a repository.
    pub git: AtomicBool,
    /// The workspace each opened conversation is in, when it is not the parent's.
    pub opened_in: Mutex<Vec<(SessionId, Option<WorkspaceId>)>>,
    /// The permission mode each conversation was opened in.
    pub opened_mode: Mutex<Vec<(SessionId, aura_security_contract::PermissionMode)>>,
    pub fail_open: AtomicBool,
    /// Conversations of the user's opened in a worktree's workspace.
    /// Worktrees added: the parent workspace, the plan and the worktree's own workspace.
    pub added: Mutex<Vec<(WorkspaceId, WorktreePlan, WorkspaceId)>>,
    /// Why a worktree cannot be placed, when a test says so.
    pub plan_refusal: Mutex<Option<String>>,
    /// The workspace's uncommitted files the next worktrees take along; none by default.
    pub carried: Mutex<Vec<String>>,
    /// What each worktree changed, by the conversation working in it.
    pub changes: Mutex<HashMap<SessionId, Vec<WorktreeChange>>>,
    /// Branches merged, in order.
    pub merged: Mutex<Vec<String>>,
    /// How the next merges end; merged with the worktree's files by default.
    pub merge_outcome: Mutex<Option<WorktreeMerge>>,
    /// Worktree folders removed, in order.
    pub removed: Mutex<Vec<String>>,
    /// The main agent's answers given to sub-agents' questions: allowed or not.
    pub answered: Mutex<Vec<(SessionId, ApprovalRequestId, ApprovalAnswer)>>,
    /// The reason given with each of those answers, in order.
    pub reasons: Mutex<Vec<String>>,
    /// Why the next answers are not given, when a test says so.
    pub answer_failure: Mutex<Option<String>>,
    /// Why the next conversations do not stop, when a test says so; they go on working.
    pub halt_failure: Mutex<Option<String>>,
    /// Why what conversations show cannot be read, when a test says so.
    pub progress_failure: Mutex<Option<String>>,
    /// The step the next merges fail in and why, when a test says so.
    pub merge_failure: Mutex<Option<(MergeStep, String)>>,
    /// Why the next worktrees are not removed, when a test says so.
    pub remove_failure: Mutex<Option<String>>,
    /// Dropped last: Windows removes the folder only once the store has closed its files.
    _root: tempfile::TempDir,
}

/// The commit a fake worktree that took files along starts its own work from.
pub const CARRIED_COMMIT: &str = "c4a1c4a1c4a1c4a1c4a1c4a1c4a1c4a1c4a1c4a1";

pub fn model(name: &str, for_subagents: bool) -> ModelOffer {
    ModelOffer { id: ModelProfileId::new(), name: name.to_owned(), for_subagents }
}

impl FakeHost {
    /// The clause of the note the last halt of `child` left for its next turn; none when that
    /// halt left none (the user stopped it).
    pub fn stop_clause(&self, child: SessionId) -> Option<String> {
        let notes = self.stop_notes.lock().unwrap();
        let (_, note) = notes.iter().rev().find(|(halted, _)| *halted == child)?;
        note.as_ref().map(|note| note.clause.clone())
    }

    pub fn new() -> Self {
        Self::with_models(vec![model("Big", false), model("Flash", true), model("Pro", true)])
    }

    pub fn with_models(models: Vec<ModelOffer>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = V2Store::create(&root.path().join("v2")).unwrap();
        Self {
            store: Mutex::new(store),
            models,
            parent_model: None,
            workspace: WorkspaceId::new(),
            opened: Mutex::default(),
            discarded: Mutex::default(),
            delivered: Mutex::default(),
            halted: Mutex::default(),
            stop_notes: Mutex::default(),
            working: Mutex::default(),
            turns: Mutex::default(),
            shown: Mutex::default(),
            fail_delivery: AtomicBool::new(false),
            delivery_failure: Mutex::default(),
            news: Arc::new(Notify::new()),
            git: AtomicBool::new(true),
            opened_in: Mutex::default(),
            opened_mode: Mutex::default(),
            fail_open: AtomicBool::new(false),
            added: Mutex::default(),
            plan_refusal: Mutex::default(),
            carried: Mutex::default(),
            changes: Mutex::default(),
            merged: Mutex::default(),
            merge_outcome: Mutex::default(),
            removed: Mutex::default(),
            answered: Mutex::default(),
            reasons: Mutex::default(),
            answer_failure: Mutex::default(),
            halt_failure: Mutex::default(),
            progress_failure: Mutex::default(),
            merge_failure: Mutex::default(),
            remove_failure: Mutex::default(),
            _root: root,
        }
    }

    pub fn team(&self) -> Team<'_, Self> {
        Team::new(self, Arc::clone(&self.news))
    }

    pub fn child(&self, index: usize) -> SessionId {
        self.opened.lock().unwrap()[index].0
    }

    pub fn last_message(&self) -> (SessionId, String) {
        let delivered = self.delivered.lock().unwrap();
        let (child, text, _) = delivered.last().unwrap();
        (*child, text.clone())
    }

    /// Sets what conversation `session` shows from now on.
    pub fn show(&self, session: SessionId, progress: Progress) {
        self.shown.lock().unwrap().insert(session, progress);
    }

    /// Reads the store, for a test that looks at the rows themselves.
    pub fn with_store<T>(
        &self,
        read: impl FnOnce(&Connection) -> StoreResult<T>,
    ) -> StoreResult<T> {
        read(self.store.lock().unwrap().database.connection())
    }

    /// Runs `sql` on the store, for a test that looks at the rows themselves.
    pub fn count(&self, sql: &str) -> i64 {
        self.store
            .lock()
            .unwrap()
            .database
            .connection()
            .query_row(sql, [], |row| row.get(0))
            .unwrap()
    }
}

impl TeamHost for FakeHost {
    fn read<T, F>(
        &self,
        _step: &'static str,
        work: F,
    ) -> impl Future<Output = anyhow::Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> StoreResult<T> + Send + 'static,
    {
        let result = work(self.store.lock().unwrap().database.connection());
        async move { Ok(result?) }
    }

    fn write<T, F>(
        &self,
        _step: &'static str,
        work: F,
    ) -> impl Future<Output = anyhow::Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> StoreResult<T> + Send + 'static,
    {
        let result = self.store.lock().unwrap().database.unit_of_work(work);
        async move { Ok(result?) }
    }

    fn models(&self) -> anyhow::Result<Vec<ModelOffer>> {
        Ok(self.models.clone())
    }

    fn parent(&self, session: SessionId) -> impl Future<Output = anyhow::Result<Parent>> + Send {
        let parent = Parent {
            session_id: session,
            workspace_id: self.workspace,
            model: self.parent_model,
            git: self.git.load(Ordering::SeqCst),
        };
        async move { Ok(parent) }
    }

    fn open(
        &self,
        _parent: &Parent,
        worktree: Option<WorkspaceId>,
        title: &str,
        model: ModelProfileId,
        mode: aura_security_contract::PermissionMode,
        _command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<SessionId>> + Send {
        let outcome = if self.fail_open.load(Ordering::SeqCst) {
            Err(anyhow::anyhow!("session.create_failed"))
        } else {
            let child = SessionId::new();
            self.opened.lock().unwrap().push((child, title.to_owned(), model));
            self.opened_in.lock().unwrap().push((child, worktree));
            self.opened_mode.lock().unwrap().push((child, mode));
            Ok(child)
        };
        async move { outcome }
    }

    fn discard(&self, child: SessionId) -> impl Future<Output = anyhow::Result<()>> + Send {
        self.discarded.lock().unwrap().push(child);
        async { Ok(()) }
    }

    fn deliver(
        &self,
        child: SessionId,
        text: String,
        command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<Delivery>> + Send {
        let outcome = if self.fail_delivery.load(Ordering::SeqCst) {
            let failure = self.delivery_failure.lock().unwrap().clone();
            Err(anyhow::anyhow!(
                failure.unwrap_or_else(|| "agent.model_binding_required".to_owned())
            ))
        } else {
            self.delivered.lock().unwrap().push((child, text, command));
            let added = !self.working.lock().unwrap().insert(child);
            if !added {
                self.turns.lock().unwrap().insert(child, TurnTaskId::new());
            }
            Ok(if added { Delivery::AddedToTurn } else { Delivery::NewTurn })
        };
        async move { outcome }
    }

    fn halt(
        &self,
        child: SessionId,
        note: Option<StopNote>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        self.stop_notes.lock().unwrap().push((child, note));
        let outcome = match self.halt_failure.lock().unwrap().clone() {
            Some(failure) => Err(anyhow::anyhow!(failure)),
            None => {
                self.halted.lock().unwrap().push(child);
                self.working.lock().unwrap().remove(&child);
                Ok(())
            }
        };
        async move { outcome }
    }

    fn progress(
        &self,
        session: SessionId,
    ) -> impl Future<Output = anyhow::Result<Progress>> + Send {
        if let Some(failure) = self.progress_failure.lock().unwrap().clone() {
            return std::future::ready(Err(anyhow::anyhow!(failure)));
        }
        let working = self.working.lock().unwrap().contains(&session);
        let turn = if working { self.turns.lock().unwrap().get(&session).copied() } else { None };
        let progress = self.shown.lock().unwrap().get(&session).cloned().unwrap_or(Progress {
            working,
            turn,
            ..Progress::default()
        });
        std::future::ready(Ok(progress))
    }

    fn answer_approval(
        &self,
        child: SessionId,
        approval: ApprovalRequestId,
        answer: ApprovalAnswer,
        reason: &str,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        if let Some(failure) = self.answer_failure.lock().unwrap().clone() {
            return std::future::ready(Err(anyhow::anyhow!(failure)));
        }
        self.answered.lock().unwrap().push((child, approval, answer));
        self.reasons.lock().unwrap().push(reason.to_owned());
        // The question is answered: the conversation shows it no more.
        if let Some(shown) = self.shown.lock().unwrap().get_mut(&child) {
            shown.reviewing.retain(|request| request.approval_id != approval);
        }
        std::future::ready(Ok(()))
    }
}

impl WorktreeHost for FakeHost {
    fn plan_worktree(
        &self,
        _workspace: WorkspaceId,
        key: &str,
    ) -> impl Future<Output = anyhow::Result<WorktreePlan>> + Send {
        let outcome = match self.plan_refusal.lock().unwrap().clone() {
            Some(refusal) => Err(anyhow::anyhow!(refusal)),
            None => Ok(WorktreePlan {
                path: format!(r"C:\Fake\AuraWorktrees\project\{key}"),
                branch: format!("aura/agent/{key}"),
                start: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            }),
        };
        async move { outcome }
    }

    fn add_worktree(
        &self,
        workspace: WorkspaceId,
        plan: &WorktreePlan,
        _command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<AddedWorktree>> + Send {
        let mut added = self.added.lock().unwrap();
        let own = match added.iter().find(|(_, earlier, _)| earlier.path == plan.path) {
            Some((_, _, own)) => *own,
            None => {
                let own = WorkspaceId::new();
                added.push((workspace, plan.clone(), own));
                own
            }
        };
        let carried = self.carried.lock().unwrap().clone();
        // Files taken along are a commit of their own on the worktree's branch.
        let base = match carried.is_empty() {
            true => plan.start.clone(),
            false => CARRIED_COMMIT.to_owned(),
        };
        async move { Ok(AddedWorktree { workspace_id: own, base, carried }) }
    }

    fn worktree_changes(
        &self,
        worktree: &Worktree,
    ) -> impl Future<Output = anyhow::Result<Vec<WorktreeChange>>> + Send {
        let changes =
            self.changes.lock().unwrap().get(&worktree.session_id).cloned().unwrap_or_default();
        async move { Ok(changes) }
    }

    fn merge_worktree(
        &self,
        worktree: &Worktree,
        _message: &str,
        _command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<WorktreeMerge>> + Send {
        if let Some((step, failure)) = self.merge_failure.lock().unwrap().clone() {
            return std::future::ready(Err(anyhow::anyhow!(failure).context(step)));
        }
        self.merged.lock().unwrap().push(worktree.details.branch.clone());
        let files: Vec<String> = self
            .changes
            .lock()
            .unwrap()
            .get(&worktree.session_id)
            .map(|changes| changes.iter().map(|change| change.path.clone()).collect())
            .unwrap_or_default();
        let outcome = self.merge_outcome.lock().unwrap().clone().unwrap_or(WorktreeMerge::Merged {
            head: "feedfacefeedfacefeedfacefeedfacefeedface".to_owned(),
            files,
        });
        std::future::ready(Ok(outcome))
    }

    fn remove_worktree(
        &self,
        worktree: &Worktree,
        _command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let outcome = match self.remove_failure.lock().unwrap().clone() {
            Some(failure) => Err(anyhow::anyhow!(failure)),
            None => {
                self.removed.lock().unwrap().push(worktree.details.path.clone());
                Ok(())
            }
        };
        async move { outcome }
    }

    fn worktree_size(&self, _worktree: &Worktree) -> Option<u64> {
        Some(4096)
    }
}

/// The tools of a conversation that has the usual table, some of them the main agent's own.
pub fn own_tools() -> Vec<String> {
    [
        "fs.read",
        "fs.list",
        "fs.find",
        "fs.search",
        "fs.write",
        "fs.patch",
        "fs.delete",
        "git.status",
        "git.commit",
        "shell.powershell",
        "shell.cmd",
        "skills.load",
        "skills.search",
        "web.fetch",
        "browser.open",
        "chrome.click",
        "computer.act",
    ]
    .map(str::to_owned)
    .to_vec()
}

pub fn call(session: SessionId, tool: &str, arguments: serde_json::Value) -> TeamCall {
    TeamCall {
        session_id: session,
        turn_task_id: TurnTaskId::new(),
        tool_call_id: ToolCallId::new(),
        tool: tool.to_owned(),
        arguments,
        own_tools: own_tools(),
    }
}

pub fn task(goal: &str) -> serde_json::Value {
    serde_json::json!({"goal": goal, "done_when": "列出相关文件和结论"})
}

/// Ends a turn of sub-agent `child` with `answer`, which is its report to the main agent.
pub async fn answered(team: &Team<'_, FakeHost>, child: SessionId, answer: &str) {
    let end = TurnEnd {
        turn_task_id: TurnTaskId::new(),
        outcome: "completed".into(),
        answer: Some(answer.to_owned()),
        reason: None,
    };
    team.turn_ended(child, end, None).await.unwrap();
}
