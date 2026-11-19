//! What the team needs from the application: the execution store, the models, and the
//! conversations its sub-agents work in.
use crate::Delivery;
use aura_domain_common::{
    ApprovalRequestId, CommandInvocationId, ModelProfileId, SessionId, TurnTaskId, UtcTimestamp,
    WorkspaceId,
};
use aura_persistence_core::StoreResult;
use rusqlite::{Connection, Transaction};
use std::future::Future;

/// Who stopped a sub-agent's turn and why, for its next turn to read: one clause that follows
/// "This turn stopped here: ", and what to do next when there is something to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopNote {
    pub clause: String,
    pub next: Option<String>,
}

/// An enabled model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOffer {
    pub id: ModelProfileId,
    pub name: String,
    /// The user allowed sub-agents to use it.
    pub for_subagents: bool,
}

/// The conversation a sub-agent is started from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parent {
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    /// The model the conversation uses.
    pub model: Option<ModelProfileId>,
    /// Whether its workspace is a git repository.
    pub git: bool,
}

/// What a conversation shows now: a sub-agent's, or the one that started them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// A turn is under way.
    pub working: bool,
    /// That turn.
    pub turn: Option<TurnTaskId>,
    /// The tool it runs now.
    pub tool: Option<String>,
    /// Approvals it waits for the user to decide.
    pub approvals: Vec<ApprovalRequestId>,
    /// Approvals it waits for the main agent to decide first.
    pub reviewing: Vec<ReviewRequest>,
    /// Tokens its answers used so far.
    pub tokens: u64,
    /// A model answer of it is under way: waiting for its endpoint or arriving.
    pub answering: bool,
    /// Its permission mode now; none when it could not be read.
    pub mode: Option<aura_security_contract::PermissionMode>,
    /// How its turn under way is going; none when no turn is.
    pub progress: Option<crate::AgentProgressView>,
}

/// A question of a sub-agent's that the main agent answers first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewRequest {
    pub approval_id: ApprovalRequestId,
    /// The tool the sub-agent called.
    pub tool: String,
    /// What the call does: the command, or the file it changes.
    pub request: String,
    /// What the sub-agent said the call is for.
    pub purpose: Option<String>,
    /// It deletes files for good.
    pub deletes: bool,
    /// The user's card offers its allowance for this question, so allow_workspace may answer it.
    pub allow_workspace: bool,
    /// When it was asked; the main agent has [`crate::REVIEW_SECONDS`] from then.
    pub asked_at: UtcTimestamp,
}

pub trait TeamHost: crate::WorktreeHost {
    /// One read of the execution store.
    fn read<T, F>(
        &self,
        step: &'static str,
        work: F,
    ) -> impl Future<Output = anyhow::Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> StoreResult<T> + Send + 'static;

    /// One write transaction of the execution store.
    fn write<T, F>(
        &self,
        step: &'static str,
        work: F,
    ) -> impl Future<Output = anyhow::Result<T>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> StoreResult<T> + Send + 'static;

    /// The enabled models, in the user's order.
    fn models(&self) -> anyhow::Result<Vec<ModelOffer>>;

    fn parent(&self, session: SessionId) -> impl Future<Output = anyhow::Result<Parent>> + Send;

    fn delegated_reads(
        &self,
        _session: SessionId,
        _turn: TurnTaskId,
        paths: Vec<String>,
    ) -> impl Future<Output = anyhow::Result<Option<crate::DelegatedReads>>> + Send {
        async move {
            anyhow::ensure!(paths.is_empty(), "agents.read_delegation_unavailable");
            Ok(None)
        }
    }

    /// Opens the hidden conversation a sub-agent works in, using `model`, in the parent's
    /// workspace or in `worktree`'s, in permission mode `mode`.
    fn open(
        &self,
        parent: &Parent,
        worktree: Option<WorkspaceId>,
        title: &str,
        model: ModelProfileId,
        mode: aura_security_contract::PermissionMode,
        command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<SessionId>> + Send;

    /// Removes a conversation opened for a sub-agent that never started.
    fn discard(&self, child: SessionId) -> impl Future<Output = anyhow::Result<()>> + Send;

    /// Gives the sub-agent a message: added to its turn, or the start of a new one.
    fn deliver(
        &self,
        child: SessionId,
        text: String,
        command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<Delivery>> + Send;

    /// Stops the sub-agent's turn, if one is under way. `note` says who stopped it and why when
    /// it was not the user, for the sub-agent's next turn to read.
    fn halt(
        &self,
        child: SessionId,
        note: Option<StopNote>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    /// What conversation `session` shows now.
    fn progress(&self, session: SessionId)
    -> impl Future<Output = anyhow::Result<Progress>> + Send;

    /// Answers sub-agent `child`'s question `approval`: allow runs this one call, allow_workspace
    /// runs it and lets the sub-agent's later deletions in its own folders go unasked, deny does
    /// not run it, and the sub-agent reads that its main agent refused it for `reason`. ask_user
    /// is never given.
    fn answer_approval(
        &self,
        child: SessionId,
        approval: ApprovalRequestId,
        answer: crate::ApprovalAnswer,
        reason: &str,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}
