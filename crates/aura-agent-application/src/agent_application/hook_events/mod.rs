//! Where the Agent loop asks the user's Hooks, and what their answers change.
//!
//! Nothing runs, and nothing is read beyond the registry, when no Hook is registered. A
//! pre-event's decision is kept before it takes effect, so a replay or a restart never asks
//! the same Hooks again.
use super::*;
use aura_extension_service::hooks::{self, HookCall, HookRunOutcome, HookRunner};
use aura_hooks::HookEvent;
use aura_persistence_v2::hooks::{self as hook_store, ToolCallHookDecision};
use std::sync::atomic::AtomicBool;

mod compaction;
mod completion;
mod message;
mod model;
mod step;
mod tools;
mod turn;
pub(super) use completion::CompletionReview;

impl AgentApplication {
    fn hook_runner(&self) -> anyhow::Result<HookRunner> {
        Ok(HookRunner {
            settings: self.settings.clone(),
            journal: self.execution.journal.clone(),
            scratch_root: hooks::scratch_root(&self.execution.journal)?,
            plugins: self.execution.plugins.get().cloned(),
        })
    }

    fn hooks_registered(&self) -> bool {
        !self.settings.hook_registry().entries.is_empty()
            || self.execution.plugins.get().is_some_and(|plugins| {
                plugins.ready().iter().any(|plugin| !plugin.package.hooks.is_empty())
            })
    }

    /// The workspace folder Hooks run in and see, when the workspace is reachable.
    fn hook_workspace_root(&self, workspace: WorkspaceId) -> Option<std::path::PathBuf> {
        let resolver = self.execution.workspace.resolver(workspace).ok()?;
        resolver.verify_root_identity().ok()?;
        let root = WorkspacePath::parse(WorkspacePath::ROOT).ok()?;
        Some(resolver.resolve_existing(&root).ok()?.absolute_path)
    }

    /// Runs one event's Hooks on a blocking worker.
    pub(super) async fn run_hooks(&self, call: HookCall) -> anyhow::Result<HookRunOutcome> {
        let runner = self.hook_runner()?;
        let powershell = self.commands.managed_runtime().await.ok();
        tokio::task::spawn_blocking(move || {
            runner.run(call, powershell.as_deref(), &AtomicBool::new(false))
        })
        .await?
    }

    fn hook_call(&self, event: HookEvent, turn: &TurnTask, payload: serde_json::Value) -> HookCall {
        HookCall {
            event,
            session_id: Some(turn.session_id),
            turn_task_id: Some(turn.turn_task_id),
            tool_call_id: None,
            tool_name: None,
            workspace_root: self.hook_workspace_root(turn.workspace_id),
            subject: None,
            payload,
        }
    }
}
