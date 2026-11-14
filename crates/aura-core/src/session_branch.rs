//! A new conversation from one finished turn of another.
//!
//! The conversation is created in the primary registry in the source's Workspace with the
//! source's permission mode. The execution store then carries the source's history into it in one
//! transaction. When that transaction fails, the new conversation is deleted again, so no half
//! branch is left and the user can simply try again.
use crate::*;
use aura_agent_domain::{AgentSessionBranchRequest, AgentSessionBranchResult};
use aura_core_broker::agent_store::StoreError;
use aura_persistence_v2::agent::branch::BranchImport;
use aura_workspace_domain::SessionCreateRequest;

pub(crate) async fn dispatch(
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> anyhow::Result<serde_json::Value> {
    let command = request
        .command_invocation_id
        .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?;
    let input: AgentSessionBranchRequest = serde_json::from_value(request.params.clone())?;
    let started = std::time::Instant::now();
    let persistence = Arc::clone(&services.persistence);
    let source = registry(&persistence, |p| p.workspace_session_projection())
        .await?
        .sessions
        .into_iter()
        .find(|s| s.session_id == input.session_id)
        .ok_or_else(|| anyhow::anyhow!("session.not_found: 这个会话已不存在。"))?;
    let session = source.session_id;
    let mode = registry(&persistence, move |p| p.session_permission_state(session)).await?.mode;
    let create = SessionCreateRequest {
        workspace_id: source.workspace_id,
        title: agent_application::branch::branch_title(&source.title),
    };
    let correlation = request.id;
    let created =
        registry(&persistence, move |p| p.create_session(command, create, mode, correlation))
            .await?;
    let created_ms = started.elapsed().as_millis();
    let import = BranchImport {
        branch: created.session_id,
        source: source.session_id,
        source_title: source.title.clone(),
        turn_task_id: input.turn_task_id,
        command_invocation_id: command,
        at: UtcTimestamp::now(),
    };
    let carried = services.agent()?.carry_into_branch(import).await;
    let origin = match carried {
        Ok(Ok(origin)) => origin,
        // The store did not answer in time: the branch may still appear, so nothing is removed.
        Ok(Err(error @ StoreError::Unknown { .. })) => {
            append_core_log(
                "session.branch_unknown",
                json!({"session_id": created.session_id, "reason": error.code()}),
            );
            anyhow::bail!(
                "agent.branch_unknown: 新建分支还没有答复，可能仍在进行；请稍后在侧边栏查看，不要重复新建。"
            )
        }
        failed => {
            // No half branch: the conversation made for it goes again.
            if let Err(cleanup) =
                crate::session_lifecycle::Purger::of(services).delete(created.session_id).await
            {
                append_core_log(
                    "session.branch_cleanup_failed",
                    json!({"session_id": created.session_id, "reason": cleanup.to_string()}),
                );
            }
            return Err(agent_application::branch::branch_refusal(failed));
        }
    };
    // The whole command's time beside the import's own (`session.branch_timing`).
    append_core_log(
        "session.branch_steps",
        json!({"created_ms": created_ms, "total_ms": started.elapsed().as_millis()}),
    );
    Ok(serde_json::to_value(AgentSessionBranchResult {
        session_id: created.session_id,
        workspace_id: created.workspace_id,
        title: created.title,
        origin,
    })?)
}

async fn registry<T: Send + 'static>(
    persistence: &Arc<PersistenceCoordinator>,
    work: impl FnOnce(&PersistenceCoordinator) -> Result<T, aura_persistence::PersistenceError>
    + Send
    + 'static,
) -> anyhow::Result<T> {
    let persistence = Arc::clone(persistence);
    Ok(tokio::task::spawn_blocking(move || work(&persistence)).await??)
}
