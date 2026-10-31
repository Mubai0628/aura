//! Planning: the whole submission is checked here, so a proposal the user is asked to
//! approve is one that can be written exactly as previewed.
use super::super::*;
use super::{ACTION, MAX_ATTEMPTS};
use aura_agent_domain::AgentTaskPurpose;
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;
use aura_workspace_domain::{InitCheck, InitViolation, WorkspaceInitCompleteInput, inspect};

/// A submission that broke rules. Every violation goes back to the model at once.
#[derive(Debug)]
pub(super) struct InitRefused {
    violations: Vec<InitViolation>,
    attempt: usize,
}
impl std::fmt::Display for InitRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "workspace_init.refused: {} problem(s), nothing was written. Fix all of them and submit again (attempt {} of {}).",
            self.violations.len(),
            self.attempt,
            MAX_ATTEMPTS
        )?;
        for violation in self.violations.iter().take(30) {
            write!(f, "\n- {violation}")?;
        }
        if self.violations.len() > 30 {
            write!(f, "\n- and {} more of the same kinds", self.violations.len() - 30)?;
        }
        Ok(())
    }
}
impl std::error::Error for InitRefused {}

/// The message the model sees when planning refuses the call. Only `/init`'s own
/// messages are passed through; they name the model's fields and workspace-relative paths.
pub(in crate::agent_execution) fn admission_feedback(error: &anyhow::Error) -> Option<String> {
    let text = error.to_string();
    text.starts_with("workspace_init.").then_some(text)
}

impl AgentExecution {
    /// Checks the submission against the rules and the workspace, then names the files it
    /// writes as ordinary workspace targets.
    pub(in crate::agent_execution) fn resolve_workspace_init(
        &self,
        action: &mut serde_json::Value,
        capabilities: &[ResolvedCapability],
        batch: &ToolBatchRequestV1,
        snapshot: &PermissionSnapshot,
    ) -> anyhow::Result<Vec<ResourceDescriptor>> {
        let id = &batch.invocation_identity;
        let tool = aura_tool_registry::agent_release::release()?
            .entry(ACTION)
            .ok_or_else(|| anyhow::anyhow!("workspace_init.unavailable"))?
            .definition
            .tool_id;
        let (purpose, refused) = {
            let store = self.journal.lock()?;
            let db = store.database.connection();
            (
                aura_persistence_v2::agent::application::purpose(db, id.agent_run_id)?,
                aura_persistence_v2::tool::execution::unsuccessful_in_turn(
                    db,
                    id.turn_task_id,
                    tool,
                )?,
            )
        };
        anyhow::ensure!(
            purpose == AgentTaskPurpose::WorkspaceInit,
            "workspace_init.not_an_init_task: workspace_init.complete only runs in a task the user started with /init"
        );
        anyhow::ensure!(
            refused < MAX_ATTEMPTS,
            "workspace_init.attempts_exhausted: {MAX_ATTEMPTS} submissions were refused. Stop submitting; finish with normal.complete and tell the user what could not be verified."
        );
        let attempt = refused + 1;
        let input: WorkspaceInitCompleteInput = serde_json::from_value(action.clone())?;
        let resolver = self.workspace.resolver(snapshot.workspace_id)?;
        // Every check runs even when an earlier one failed: one refusal names every problem.
        let InitCheck { plan, mut violations } =
            inspect(&input, &forbidden_text(resolver.canonical_root()));
        violations.extend(super::evidence::verify(self, id, snapshot, &resolver, &plan)?);
        let previous = match super::evidence::targets(self, snapshot, &input, &plan)? {
            Ok(previous) => previous,
            Err(stale) => {
                violations.extend(stale);
                Vec::new()
            }
        };
        if !violations.is_empty() {
            return Err(InitRefused { violations, attempt }.into());
        }
        // The approval binds the exact bytes that will be written, and what they replace.
        *action = serde_json::to_value(super::InitAction {
            files: plan
                .files
                .iter()
                .zip(previous)
                .map(|(file, previous)| super::InitActionFile {
                    path: file.path.clone(),
                    content: file.content.clone(),
                    previous,
                })
                .collect(),
            input,
        })?;
        let mut resources = Vec::new();
        for file in &plan.files {
            let mut target = serde_json::json!({ "path": file.path });
            let mut needed = capabilities.to_vec();
            let resolved = self
                .resolve_file_target(H::FileWrite, &mut target, &mut needed, batch, snapshot)
                .map_err(|_| {
                    anyhow::anyhow!(
                        "workspace_init.protected_target: {} is not an ordinary workspace file; /init cannot write it.",
                        file.path
                    )
                })?;
            anyhow::ensure!(
                needed == capabilities
                    && matches!(resolved.as_slice(), [ResourceDescriptor::WorkspacePath { .. }]),
                "workspace_init.protected_target: {} is not an ordinary workspace file; /init cannot write it.",
                file.path
            );
            resources.extend(resolved);
        }
        Ok(resources)
    }
}

/// Local facts that must not appear in the files: this workspace's absolute location, the
/// profile directory, and the Windows account where it names a path or an address. A bare
/// account name is not matched: names such as "dev" or "build" are ordinary command words.
fn forbidden_text(root: &std::path::Path) -> Vec<String> {
    let mut forbidden = vec![root.display().to_string()];
    if let Ok(profile) = std::env::var("USERPROFILE") {
        forbidden.push(profile);
    }
    if let Ok(name) = std::env::var("USERNAME") {
        forbidden.extend([format!("\\{name}\\"), format!("/{name}/"), format!("{name}@")]);
    }
    forbidden
}
