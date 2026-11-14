//! Sub-agents in the execution path: the calls a sub-agent is refused before anything starts, and
//! the team tools' calls handed to the application through the team port.
use super::*;
use aura_agent_team::{MemberState, TeamCall, policy, store as team_store};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Boxed: every call of every conversation passes here, and the dispatch futures and their
/// debug-build frames then grow by a pointer, not by this state.
type Boxed<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// Whether `action_kind` is a team tool.
pub(super) fn is_team_tool(action_kind: &str) -> bool {
    action_kind.starts_with("agents.") || action_kind == policy::REPORT_TOOL
}

impl AgentExecution {
    /// Why a call of a sub-agent's conversation may not start: a tool only the main agent has or
    /// one it was narrowed away from, another conversation, another workspace, Aura's data, or any
    /// call after its final report. None for every other conversation's call. Everything else is
    /// its permission mode and rules, as for the main agent.
    pub(super) fn subagent_refusal(
        self: &Arc<Self>,
        call: &CallContext,
    ) -> Boxed<anyhow::Result<Option<String>>> {
        let service = Arc::clone(self);
        let checked = call.clone();
        let step =
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "agent.dispatch.subagent");
        Box::pin(async move {
            let sections = Arc::clone(&service);
            sections
                .section(step, StoreAccess::Read, move || service.subagent_refusal_sync(&checked))
                .await
        })
    }

    fn subagent_refusal_sync(&self, call: &CallContext) -> anyhow::Result<Option<String>> {
        let session = call.snapshot.session_id;
        let member =
            team_store::member_by_child(self.journal.lock()?.database.connection(), session)?;
        let Some(member) = member else { return Ok(None) };
        let content = &call.operation.content;
        let tool = content.action_kind.as_str();
        if member.state != MemberState::Running && tool != policy::REPORT_TOOL {
            return Ok(Some(
                "agents.reported: 你已经向主 Agent 提了问题（或已被停止），这一轮不能再调用工具。用一两句话结束这一轮。"
                    .to_owned(),
            ));
        }
        if let Some(refusal) =
            policy::refused_call(tool, &content.action, member.details.grant.tools.as_deref())
        {
            return Ok(Some(refusal.to_string()));
        }
        // Another workspace - the main one, or another sub-agent's worktree - is never its own:
        // the main agent merges its work in.
        let own = call.snapshot.workspace_id;
        if content.resources.iter().any(|resource| match resource {
            ResourceDescriptor::WorkspaceRoot { workspace_id }
            | ResourceDescriptor::WorkspacePath { workspace_id, .. } => *workspace_id != own,
            _ => false,
        }) {
            return Ok(Some(policy::other_workspace_refusal().to_string()));
        }
        for resource in &content.resources {
            let refused = match resource {
                // A path in Aura's data is classed as a sensitive or a system location, never an
                // ordinary one; all three are checked. A read in the folder of a Skill usable now
                // stays ordinary, as for the main agent: Aura's store and plugins keep their
                // Skills there.
                ResourceDescriptor::ExternalPath { canonical_path, .. }
                | ResourceDescriptor::SensitiveUserPath { canonical_path, .. }
                | ResourceDescriptor::SystemPath { canonical_path, .. } => {
                    let path = Path::new(&canonical_path.display);
                    // Listing the Skills reads their folders: only for a path in Aura's data.
                    let skill_read = || {
                        matches!(resource, ResourceDescriptor::ExternalPath { .. })
                            && matches!(tool, "fs.read" | "fs.list" | "fs.find" | "fs.search")
                            && aura_windows_platform::inspect_existing_path(path, true).is_ok_and(
                                |info| {
                                    self.in_usable_skill_folder(call.snapshot.workspace_id, &info)
                                },
                            )
                    };
                    (self.control_resources.in_aura_data(path) && !skill_read())
                        .then(policy::data_refusal)
                }
                _ => None,
            };
            if let Some(refusal) = refused {
                return Ok(Some(refusal.to_string()));
            }
        }
        if matches!(tool, "shell.powershell" | "shell.cmd") {
            let text = ["script", "cwd"]
                .iter()
                .filter_map(|field| content.action.get(*field).and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            let folders: Vec<PathBuf> = self.control_resources.data_root_paths();
            if policy::names_aura_data(&text, &folders, &|name| std::env::var(name).ok()).is_some()
            {
                return Ok(Some(policy::data_refusal().to_string()));
            }
        }
        Ok(None)
    }
}

/// Runs a team call through the port and records its result. Only merging and discarding a
/// worktree write outside Aura's own records, through the native Git writer: a Git write whose
/// end was not seen stays an unknown effect and is never run again by itself.
pub(super) fn invoke(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> Boxed<anyhow::Result<()>> {
    Box::pin(invoke_unboxed(service, call, request, start_lease))
}

async fn invoke_unboxed(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> anyhow::Result<()> {
    let verifier = Arc::clone(&service);
    let verified = call.clone();
    let operation = request.operation.operation_id;
    service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "agents.verify_start"),
            StoreAccess::Read,
            move || {
                verifier.verify_source(&verified)?;
                aura_persistence_v2::agent::checkpoint::verify_start_operation(
                    verifier.journal.lock()?.database.connection(),
                    operation,
                )?;
                Ok(())
            },
        )
        .await?;
    drop(start_lease);
    let team_call = TeamCall {
        session_id: call.snapshot.session_id,
        turn_task_id: call.batch.invocation_identity.turn_task_id,
        tool_call_id: call.validated.call.tool_call_id,
        tool: request.operation.content.action_kind.clone(),
        arguments: request.operation.content.action.clone(),
        own_tools: call
            .registry
            .catalog_entries
            .iter()
            .map(|entry| entry.canonical_name.to_string())
            .collect(),
    };
    let answered = match service.team.tools() {
        Some(tools) => tools.call(team_call).await,
        None => Err("agents.unavailable: 子 Agent 现在不可用，请稍后再试。".to_owned()),
    };
    let (success, output, error) = match answered {
        Ok(output) => (true, Some(output), None),
        Err(error) => (false, None, Some(error.chars().take(4_000).collect::<String>())),
    };
    // A Git write whose end was not seen, or whose process was not seen to stop, may have
    // changed the workspace.
    let unknown = error.as_deref().is_some_and(|error| {
        error.contains("git.outcome_unknown") || error.contains("git.stop_unconfirmed")
    });
    let rejection = error.as_deref().filter(|_| !unknown).and_then(refusal);
    let evidence = BrokerEvidence {
        action_kind: request.operation.content.action_kind.clone(),
        request_fingerprint: request.operation.fingerprint.0.clone(),
        // A refusal is answered before the team writes anything; reading writes nothing.
        side_effect: match (success, request.operation.content.action_kind.as_str()) {
            _ if unknown => SideEffectStateV1::Unknown,
            (false, _) | (true, "agents.status" | "agents.result") => SideEffectStateV1::NoEffect,
            (true, _) => SideEffectStateV1::FullyApplied,
        },
        before: request.plan.prepared.preparation["domain"].clone(),
        after: Value::Null,
        domain_journal: json!({
            "schema": "agent-team-v1",
            "output": output,
            "error": error,
            "operation_rejection": rejection,
        }),
    };
    let owner = Arc::clone(&service);
    service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "agents.result"),
            StoreAccess::Write,
            move || {
                Ok(owner.journal.lock()?.database.unit_of_work(|tx| {
                    aura_persistence_v2::execution::broker_result::record(
                        tx,
                        &request,
                        &evidence,
                        success,
                        UtcTimestamp::now(),
                    )
                })?)
            },
        )
        .await?;
    Ok(())
}

/// A refusal of the call's own input, or of a sub-agent the conversation does not have, as the
/// rejection its row and the model read; any other failure stays the tool's.
fn refusal(error: &str) -> Option<Value> {
    let code = error.split(':').next()?.trim();
    let category = aura_agent_team::refusal_category(code)?;
    Some(json!({"code": code, "category": category, "message": error, "retryable": false}))
}
